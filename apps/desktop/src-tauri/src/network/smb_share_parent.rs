//! "Show “..” in the top folder" for an SMB share: in a share's top folder, `..`
//! opens the server's list of shares (#1238). This module answers the two questions
//! the UI has about a share, both by volume id: where `..` leads (which server, under
//! what name, back onto which share), and what the share's own switch is set to.
//!
//! ❗ **One default plus per-share overrides, and an override is stored only while it
//! differs from the default.** The default is the frontend setting
//! `network.showShareParentRow`, which the caller hands in on every write, so flipping
//! a share back to match the default clears its entry and the row menu's plain
//! checkbox stays honest without a "use default" state. A default flipped later keeps
//! every override: each one is a choice someone made about that share.
//!
//! Overrides live in `known-shares.json` (`known_shares::parent_row_overrides`), keyed
//! by server + share and matched by `server_identity::same_machine` like the
//! direct-connection opt-outs, ❌ never by account: a share macOS mounted has no saved
//! row, and `statfs` spells one server several ways.

use crate::network::known_shares::{self, ShareRef, names_share};
use crate::network::manual_servers::{create_network_host, typed_name};
use crate::network::server_identity::{SmbServer, smb_server, smb_servers_of};
use crate::network::smb_direct_switch::{ShareRead, read_share_within};
use crate::network::smb_server_address::{ServerAddress, friendly_server_name, resolve_server_address};
use crate::network::smb_upgrade::MOUNT_READ_LIMIT;
use crate::network::{HostSource, NetworkHost, service_name_to_hostname};
use crate::volumes::{SmbMountInfo, get_smb_mount_info};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// One share's own "Show “..”" choice, stored only while it differs from the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParentRowOverride {
    #[serde(flatten)]
    share: ShareRef,
    show: bool,
}

/// Where `..` in a share's top folder leads, and the share's own switch.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareParent {
    /// The server, as its share list opens it.
    pub host: NetworkHost,
    /// What to call the server in words: the name a person typed for it, else the
    /// name it goes by, with its port off 445. The same name the share's own label uses.
    pub server_label: String,
    /// The share as the mount spells it, for the cursor to land back on.
    pub share_name: String,
    /// The share's own choice, or `None` while it follows the default.
    pub show_parent_row: Option<bool>,
}

/// Where `..` leads out of the share behind `volume_id`, or `None` when no SMB share
/// is behind it (or its mount didn't answer in time).
pub(crate) async fn share_parent_for(volume_id: &str) -> Option<ShareParent> {
    share_parent_within(volume_id, MOUNT_READ_LIMIT, get_smb_mount_info).await
}

async fn share_parent_within(
    volume_id: &str,
    limit: Duration,
    read: impl FnOnce(&str) -> Option<SmbMountInfo> + Send + 'static,
) -> Option<ShareParent> {
    let ShareRead::Smb(info) = read_share_within(volume_id, limit, read).await else {
        return None;
    };
    // Discovery is read BEFORE the store's lock is taken, so the two never nest.
    let hosts = crate::network::fresh_discovered_hosts();
    let show_parent_row =
        known_shares::read_parent_row_overrides(|overrides| override_in(overrides, &info.server, &info.share, &hosts));
    Some(ShareParent {
        host: host_for_mount(&info, &hosts),
        server_label: server_label(&info),
        share_name: info.share,
        show_parent_row,
    })
}

/// Saves the share's switch, given the default it's measured against. Answers `false`
/// when no SMB share is behind `volume_id` (or its mount didn't answer), and then
/// nothing was saved.
pub(crate) async fn set_parent_row_for(volume_id: &str, show: bool, default_show: bool) -> bool {
    set_parent_row_within(volume_id, show, default_show, MOUNT_READ_LIMIT, get_smb_mount_info).await
}

async fn set_parent_row_within(
    volume_id: &str,
    show: bool,
    default_show: bool,
    limit: Duration,
    read: impl FnOnce(&str) -> Option<SmbMountInfo> + Send + 'static,
) -> bool {
    let ShareRead::Smb(info) = read_share_within(volume_id, limit, read).await else {
        return false;
    };
    let hosts = crate::network::fresh_discovered_hosts();
    known_shares::update_parent_row_overrides(|overrides| {
        apply_choice(overrides, &info.server, &info.share, show, default_show, &hosts);
    });
    true
}

/// The share's override in `overrides`, under any name its server goes by.
fn override_in(overrides: &[ParentRowOverride], server: &str, share: &str, hosts: &[NetworkHost]) -> Option<bool> {
    overrides
        .iter()
        .find(|entry| names_share(&entry.share, &[server], share, hosts))
        .map(|entry| entry.show)
}

/// Records the share's choice: every entry naming the share goes first (so a share
/// chosen under two spellings keeps one entry), and a new one is filed only when
/// `show` differs from `default_show`.
fn apply_choice(
    overrides: &mut Vec<ParentRowOverride>,
    server: &str,
    share: &str,
    show: bool,
    default_show: bool,
    hosts: &[NetworkHost],
) {
    overrides.retain(|entry| !names_share(&entry.share, &[server], share, hosts));
    if show != default_show {
        overrides.push(ParentRowOverride {
            share: ShareRef {
                server_name: server.to_string(),
                share_name: share.to_string(),
            },
            show,
        });
    }
}

/// The server a mount is from, as the share list opens it.
///
/// A host discovery knows is preferred, a resolved one first, so the list reads the
/// same cached shares and sign-in the servers hub does (both key on the host). With
/// none, the host is made from the mount's own server the way "Add server" makes one,
/// so `..` works with discovery off too.
fn host_for_mount(info: &SmbMountInfo, hosts: &[NetworkHost]) -> NetworkHost {
    let mounted = SmbServer::new(&info.server, info.port);
    let serves = |host: &&NetworkHost| smb_servers_of(host).iter().any(|server| server.is(&mounted, hosts));
    let rank = |host: &NetworkHost| match (host.hostname.is_some(), host.source) {
        (true, HostSource::Discovered) => 0,
        (true, HostSource::Manual) => 1,
        (false, _) => 2,
    };
    let mut matching: Vec<&NetworkHost> = hosts.iter().filter(serves).collect();
    matching.sort_by_key(|host| rank(host));
    matching
        .first()
        .map(|host| (*host).clone())
        .unwrap_or_else(|| made_host(info))
}

/// A host for a mount discovery doesn't know: dialable by the address the mount's
/// server resolves to, or, for an mDNS service name nothing has resolved yet, by its
/// `.local` hostname.
fn made_host(info: &SmbMountInfo) -> NetworkHost {
    let address = match resolve_server_address(&info.server) {
        ServerAddress::Connectable(address) => address,
        ServerAddress::UndiscoveredService => service_name_to_hostname(&friendly_server_name(&info.server)),
    };
    create_network_host(&address, info.port)
}

/// The server's name in words, the way `smb_share_label::mounted` names it.
fn server_label(info: &SmbMountInfo) -> String {
    typed_name(&[SmbServer::new(&info.server, info.port)])
        .unwrap_or_else(|| smb_server(&friendly_server_name(&info.server), info.port))
}

#[cfg(test)]
#[path = "smb_share_parent_test.rs"]
mod tests;
