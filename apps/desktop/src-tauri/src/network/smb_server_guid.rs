//! Which server each SMB share is on: recording the GUID a direct session's
//! NEGOTIATE answered, and probing the saved shares that never had one.
//!
//! The GUID is what proves two saved shares (one NAS by its LAN and its Tailscale
//! address) are one share (`location::same_share`). Two writers, two strengths
//! (`server_guid::GuidProof`):
//!
//! - **A direct session comes up** ([`record_session_guid`], from both
//!   `smb_upgrade` install paths): the session's GUID goes onto the share's saved
//!   row, overwriting when it signed in as an account, filling only for a guest.
//!   A live share with no saved row keeps it here in memory, and a row saved later
//!   adopts it ([`adopt_live_guid`]).
//! - **The background probe** ([`schedule_probe`]): a NEGOTIATE-only dial of a
//!   saved address, no credentials, for rows saved before Cmdr kept GUIDs. Runs
//!   only when two or more share rows a mount went through share a share name
//!   and one lacks a GUID (never a host nobody connected), one pass at a time, each dial bounded by [`PROBE_TIMEOUT`], and only ever
//!   FILLS. ❌ Never from a tool handler: a model's read must not dial a host
//!   (pinned by `probe_tests::no_tool_handler_reaches_the_probe`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crate::network::server_guid::ServerGuid;
use cmdr_fs::log_private::LogPrivate;

use crate::ignore_poison::IgnorePoison;
use crate::network::known_shares::{self, KnownNetworkShare};
use crate::network::server_guid::GuidProof;

/// How long one probe may wait on a saved address: the TCP connect and the
/// NEGOTIATE reply each. A Tailscale address that's down costs this once, in the
/// background.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// How long after startup the first probe pass waits, so it never competes with
/// the startup listing and the mount upgrades for the network.
const STARTUP_SETTLE: Duration = Duration::from_secs(20);

/// A live direct session's server, for a share no saved row holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiveShare {
    pub server_guid: ServerGuid,
    pub proof: GuidProof,
    pub share_name: String,
    /// The account the session signed in as, `None` for a guest.
    pub username: Option<String>,
}

/// Live sessions' servers, by volume id. Read only for a volume that's live, so
/// an entry outliving its volume is never consulted.
static LIVE: LazyLock<Mutex<HashMap<String, LiveShare>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The server a live share with no saved row is on, if its session told us.
pub(crate) fn live_share(volume_id: &str) -> Option<LiveShare> {
    LIVE.lock_ignore_poison().get(volume_id).cloned()
}

/// Records the server GUID of the direct session `volume` just brought up for
/// `volume_id`. A session signed in as an account proves its server; a guest's
/// only fills.
pub(crate) async fn record_session_guid(
    volume: &cmdr_smb::volume::SmbVolume,
    volume_id: &str,
    share_name: &str,
    username: Option<&str>,
) {
    // The session's NEGOTIATE answer, off the same snapshot the diagnostics
    // dashboard reads: no I/O, a brief lock.
    let negotiated = volume.diagnostics().await.and_then(|d| d.primary.negotiated);
    let Some(server_guid) = negotiated.and_then(|n| ServerGuid::from_negotiated(n.server_guid)) else {
        return;
    };
    record_negotiated_guid(volume_id, share_name, server_guid, username);
}

/// [`record_session_guid`] once the session's GUID is read: onto the saved row, whose
/// own account decides the proof (`known_shares::record_server_guid`), and into
/// [`LIVE`], where the session vouches for itself.
fn record_negotiated_guid(volume_id: &str, share_name: &str, server_guid: ServerGuid, username: Option<&str>) {
    let username = username.filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case("guest"));
    let proof = if username.is_some() {
        GuidProof::Authenticated
    } else {
        GuidProof::Unauthenticated
    };
    let saved = known_shares::record_server_guid(volume_id, &server_guid, username);
    log::debug!(
        target: "smb_server_guid",
        "{volume_id}: session answered server GUID {} (session {proof:?}, saved row: {saved})",
        server_guid.as_str()
    );
    LIVE.lock_ignore_poison().insert(
        volume_id.to_string(),
        LiveShare {
            server_guid,
            proof,
            share_name: share_name.to_string(),
            username: username.map(str::to_string),
        },
    );
}

/// A share row was just saved for `volume_id`: give it the GUID its live session
/// already answered (the upgrade can land before the mount's row does), then
/// probe whatever rows still lack one.
pub(crate) fn adopt_live_guid(volume_id: &str) {
    if let Some(live) = live_share(volume_id) {
        // The row's own account decides the proof: a session signed in as someone
        // else, or a guest, only fills.
        let saved = known_shares::record_server_guid(volume_id, &live.server_guid, live.username.as_deref());
        log::debug!(target: "smb_server_guid", "{volume_id}: adopted its live session's GUID (saved row: {saved})");
    }
    schedule_probe(Duration::ZERO);
}

/// The saved shares a probe pass dials: rows with no GUID whose share name another
/// share row also has, so a GUID could prove them one share. Rows already holding
/// a GUID are never dialed: a probe can't change them anyway. Pure.
///
/// ❗ Only rows a mount went through (`volume_id`) count, as a target or as a twin:
/// a share an add named (an approved Ask Cmdr suggestion included) is on a host
/// nobody connected yet, and Cmdr doesn't dial one on its own initiative.
pub(crate) fn rows_to_probe(rows: &[KnownNetworkShare]) -> Vec<&KnownNetworkShare> {
    let fold = crate::network::share_choices::fold_name;
    let has_guid = |row: &KnownNetworkShare| row.server_guid.as_deref().and_then(ServerGuid::parse).is_some();
    let connected = |row: &&KnownNetworkShare| row.is_share() && row.volume_id.is_some();
    rows.iter()
        .filter(connected)
        .filter(|row| !has_guid(row))
        .filter(|row| {
            rows.iter()
                .filter(connected)
                .filter(|other| fold(&other.share_name) == fold(&row.share_name))
                .count()
                >= 2
        })
        .collect()
}

/// One probe pass at a time: a pass already running covers every trigger that
/// arrives meanwhile.
static PROBING: AtomicBool = AtomicBool::new(false);

/// Starts a background probe pass after `delay`, unless one is already running or
/// nothing qualifies. Startup calls it once ([`schedule_startup_probe`]); a saved
/// share row calls it through [`adopt_live_guid`].
pub(crate) fn schedule_probe(delay: Duration) {
    if !crate::network::is_network_enabled() || !crate::test_mode::may_adopt_preexisting_network_mounts() {
        return;
    }
    if rows_to_probe(&known_shares::saved_shares()).is_empty() {
        return;
    }
    if PROBING.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        probe_pass().await;
        PROBING.store(false, Ordering::Release);
    });
}

/// The startup pass, once the startup listing and the mount upgrades settled.
pub(crate) fn schedule_startup_probe() {
    schedule_probe(STARTUP_SETTLE);
}

/// A probe's answer, onto the saved row: no session signed in, so it only fills.
fn record_probed_guid(place: &str, guid: &ServerGuid) -> bool {
    known_shares::record_server_guid(place, guid, None)
}

async fn probe_pass() {
    let rows: Vec<KnownNetworkShare> = rows_to_probe(&known_shares::saved_shares())
        .into_iter()
        .cloned()
        .collect();
    for row in rows {
        let place = known_shares::place_id(&row);
        let host = row.address.as_deref().unwrap_or(&row.server_name);
        let port = row.port.unwrap_or(445);
        let probed = cmdr_smb::connection::probe_server_guid(host, port, PROBE_TIMEOUT).await;
        match probed.map(ServerGuid::from_negotiated) {
            Ok(Some(guid)) => {
                let saved = record_probed_guid(&place, &guid);
                log::debug!(
                    target: "smb_server_guid",
                    "{place}: probe read server GUID {} (saved row: {saved})",
                    guid.as_str()
                );
            }
            Ok(None) => log::debug!(target: "smb_server_guid", "{place}: the server answered no GUID"),
            Err(e) => log::debug!(
                target: "smb_server_guid",
                "{place}: probe of {:?} didn't answer: {e}",
                LogPrivate::host(host)
            ),
        }
    }
}

#[cfg(test)]
#[path = "smb_server_guid_test.rs"]
mod probe_tests;
