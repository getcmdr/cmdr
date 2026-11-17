//! NFS volume wiring: turns "the user asked for this export" into a registered
//! `NfsVolume`.
//!
//! ❗ **A backend never registers itself.** The same shape as
//! `s3_volume_wiring`: dial, register (retiring any predecessor), and remember
//! the place for next time, in that order. The rule:
//! `network/DETAILS.md` § "Backends never register themselves".
//!
//! ❗ **The id comes from what the dial RESOLVED, ❌ never from what was typed.**
//! `cmdr_nfs::connect_nfs_volume` splits a typed `/share/Public/photos` into the
//! export and a folder under it, so this registers under the returned volume's
//! id and saves its params (the real export, the rest as the start folder).
//!
//! No secret anywhere: `AUTH_SYS` sends a uid and a gid, which are settings of
//! the place in `nfs_known_places.rs`.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use cmdr_fs::volume::Volume;
use cmdr_nfs::{NfsConnectError, NfsConnectionParams, NfsVolume};
use tokio_util::sync::CancellationToken;

use super::connect_wiring::{self, AttemptTable};
use super::live_server_edit::{self, ConnectedPlace, PlaceEdit};
use super::nfs_known_places::{self, KnownNfsPlace};
use super::saved_server_fields::{self, SavedServerOutcome};

/// What a connect attempt produced, in the terms the add form branches on.
///
/// ❗ Every outcome is a variant, including the ones that look like failures,
/// and ❌ none may be recovered from a message.
#[derive(Debug, PartialEq, Eq)]
pub enum NfsConnection {
    /// A live volume, registered under `volume_id`: the RESOLVED export's id.
    Connected {
        /// The id every listing and tab is filed under.
        volume_id: String,
    },
    /// The server has no export by that path.
    ExportNotFound,
    /// The server refused this Mac the export, from a reserved port.
    ExportRefused,
    /// The export refused us, and this process couldn't bind a port below 1024.
    ReservedPortRequired,
    /// The server speaks no NFSv3 over TCP.
    NoNfsV3,
    /// The dial didn't finish inside its budget.
    TimedOut,
    /// No route, refused, DNS, or a transport-level breakdown.
    Unreachable,
    /// The user called it off. Nothing was registered, remembered, or stored.
    Cancelled,
}

/// A dial that came up: the volume, and the params it RESOLVED (the real
/// export, the start folder the split left).
pub(crate) struct Dialed {
    pub volume: Arc<dyn Volume>,
    pub params: NfsConnectionParams,
}

/// The backend half the wiring needs, as a seam so the lifecycle is testable
/// without a server ([`CrateNfs`] is the only production one).
pub(crate) trait NfsBackend: Send + Sync {
    /// Dials `params` (whose export may carry a folder) as a volume named `name`.
    fn dial<'a>(
        &'a self,
        name: &'a str,
        params: NfsConnectionParams,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Dialed, NfsConnectError>> + Send + 'a>>;
    /// The params a registered volume dials with, or `None` when it isn't one of
    /// this backend's.
    fn params_of(&self, volume: &dyn Volume) -> Option<NfsConnectionParams>;
    /// Another instance of `volume` named `name`, riding its connection.
    fn sharing_connection(&self, volume: &dyn Volume, name: &str) -> Option<Arc<dyn Volume>>;
    /// Moves the live volume's "reconnect automatically" switch.
    fn set_auto_reconnect(&self, volume: &dyn Volume, on: bool);
    /// Moves the start folder the live volume answers in its params, with no
    /// redial: what a start-folder edit to a connected place saves.
    fn set_start_folder(&self, volume: &dyn Volume, folder: Option<String>);
}

/// `cmdr_nfs` itself.
pub(crate) struct CrateNfs;

impl NfsBackend for CrateNfs {
    fn dial<'a>(
        &'a self,
        name: &'a str,
        params: NfsConnectionParams,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Dialed, NfsConnectError>> + Send + 'a>> {
        Box::pin(async move {
            let volume = cmdr_nfs::connect_nfs_volume(name, params, crate::volume_host::host(), cancel).await?;
            Ok(Dialed {
                params: volume.params(),
                volume: Arc::new(volume),
            })
        })
    }

    fn params_of(&self, volume: &dyn Volume) -> Option<NfsConnectionParams> {
        volume.as_any().downcast_ref::<NfsVolume>().map(NfsVolume::params)
    }

    fn sharing_connection(&self, volume: &dyn Volume, name: &str) -> Option<Arc<dyn Volume>> {
        let nfs = volume.as_any().downcast_ref::<NfsVolume>()?;
        Some(Arc::new(nfs.sharing_connection(name)))
    }

    fn set_auto_reconnect(&self, volume: &dyn Volume, on: bool) {
        if let Some(nfs) = volume.as_any().downcast_ref::<NfsVolume>() {
            nfs.set_auto_reconnect(on);
        }
    }

    fn set_start_folder(&self, volume: &dyn Volume, folder: Option<String>) {
        if let Some(nfs) = volume.as_any().downcast_ref::<NfsVolume>() {
            nfs.set_start_folder(folder);
        }
    }
}

/// The connect attempts a user could still call off (`connect_wiring.rs`).
pub(super) static ATTEMPTS: AttemptTable = AttemptTable::new("an nfs");

/// Calls off the connect filed under `attempt_id`, answering whether one was
/// running.
pub fn cancel_connect(attempt_id: &str) -> bool {
    ATTEMPTS.cancel(attempt_id)
}

/// Calls off every connect dialing `place`, for a move that took it to a new
/// address (`server_move.rs`). Answers how many were running.
pub fn cancel_dials_to(place: &str) -> usize {
    ATTEMPTS.cancel_dials_to(place)
}

/// Dials the place `place` describes (its export as typed, folder and all), and
/// on success registers the volume and remembers the place under the RESOLVED
/// export.
///
/// `display_name` is the SERVER's name as an add typed it, adopted once the dial
/// lands; a blank one leaves the server's name alone
/// (`nfs_known_places::adopt_typed_name`). ❗ A cancelled connect leaves nothing
/// behind.
pub async fn connect_and_register(display_name: &str, place: KnownNfsPlace, attempt_id: &str) -> NfsConnection {
    connect_and_register_since(connect_wiring::DialTicket::now(), display_name, place, attempt_id).await
}

/// [`connect_and_register`] for `place` as the store holds it, read at
/// `set_out`, which was taken BEFORE that read (`connect_wiring::DialTicket`).
pub async fn connect_and_register_since(
    set_out: connect_wiring::DialTicket,
    display_name: &str,
    place: KnownNfsPlace,
    attempt_id: &str,
) -> NfsConnection {
    connect_and_register_with(&CrateNfs, set_out, display_name, place, attempt_id).await
}

pub(crate) async fn connect_and_register_with(
    backend: &dyn NfsBackend,
    set_out: connect_wiring::DialTicket,
    display_name: &str,
    place: KnownNfsPlace,
    attempt_id: &str,
) -> NfsConnection {
    let place = KnownNfsPlace {
        start_folder: export_relative(place.start_folder.as_deref()),
        // A first connect pins the new place; `remember` keeps the stored pin
        // for a place already saved.
        pinned: true,
        ..place
    };
    // The TYPED export's id: all a Cancel or a move can name before the split.
    let (cancel, attempt) = ATTEMPTS.register_dialing(attempt_id, vec![place.volume_id()], set_out);
    let name = server_name_for(display_name, &place);
    let dialed = match backend
        .dial(
            &nfs_known_places::place_label_named(&name, &place),
            place.params(),
            cancel,
        )
        .await
    {
        Ok(dialed) => dialed,
        Err(e) => return failed(e),
    };
    let resolved = KnownNfsPlace::from_params(&dialed.params, place.pinned, crate::timestamps::now_rfc3339());
    let volume_id = resolved.volume_id();
    // The split may have named a shorter export than the one typed, and the
    // volume reads as the export it IS.
    let label = nfs_known_places::place_label_named(&name, &resolved);
    let volume = if dialed.volume.name() == label {
        dialed.volume
    } else {
        backend
            .sharing_connection(&*dialed.volume, &label)
            .unwrap_or(dialed.volume)
    };
    // A move landed while this dial was out: remembering the place here would
    // save the old address again (`connect_wiring.rs`).
    let Some(_landing) = attempt.land().await else {
        connect_wiring::let_go(volume).await;
        return NfsConnection::Cancelled;
    };
    connect_wiring::install_retiring_incumbent(&volume_id, volume).await;
    nfs_known_places::adopt_typed_name(&resolved, display_name);
    nfs_known_places::remember(resolved);
    log::info!(target: "volume", "registered NFS volume {volume_id}");
    NfsConnection::Connected { volume_id }
}

/// The typed connect errors, widened into the outcome the frontend branches on.
/// The diagnostic strings stay in the log.
fn failed(error: NfsConnectError) -> NfsConnection {
    match error {
        NfsConnectError::ExportNotFound => NfsConnection::ExportNotFound,
        // The server is there and turned this Mac away. From a connect, a server
        // that won't list its exports had the typed path tried by `MNT` instead,
        // so a listing refusal reaching here is the same "no" as the mount's.
        NfsConnectError::ExportRefused | NfsConnectError::ListingRefused => NfsConnection::ExportRefused,
        NfsConnectError::ReservedPortRequired => NfsConnection::ReservedPortRequired,
        NfsConnectError::NoNfsV3 => NfsConnection::NoNfsV3,
        NfsConnectError::TimedOut => NfsConnection::TimedOut,
        NfsConnectError::Cancelled => NfsConnection::Cancelled,
        NfsConnectError::Unreachable(what) | NfsConnectError::Transport(what) => {
            log::info!(target: "volume", "an nfs connection didn't come up: {what}");
            NfsConnection::Unreachable
        }
    }
}

/// A start folder as the store keeps it: export-relative, normalized, and `None`
/// for the export's root.
fn export_relative(folder: Option<&str>) -> Option<String> {
    // Every absolute path is under `/`, so the refusal can't happen.
    saved_server_fields::start_folder_under_root("/", folder).unwrap_or_default()
}

/// The server's name this dial or edit goes by: the one typed, else the one
/// saved (a blank Name leaves it alone: `nfs_known_places::adopt_typed_name`).
fn server_name_for(typed: &str, place: &KnownNfsPlace) -> String {
    if saved_server_fields::is_named(typed) {
        typed.trim().to_string()
    } else {
        nfs_known_places::server_name(place)
    }
}

/// Saves a place without dialing it: an edit, or an add that doesn't connect.
///
/// ❗ The saved entry is the durable copy and a CONNECTED place holds a live one,
/// so an edit moves both. A changed name, start folder, or switch rides the live
/// connection (`live_server_edit.rs`), ❌ no redial. ❗ A changed uid or group is a
/// NEW connection: every call's `AUTH_SYS` credential carries them, so the new
/// identity dials its own, confirms the start folder through it, and replaces
/// the live one through `install_retiring_incumbent` (transfers on the old one
/// finish there). A refusal at any step moves NOTHING, the store included.
///
/// `display_name` is the SERVER's name; a blank one leaves it alone.
pub async fn save_without_connecting(place: KnownNfsPlace, display_name: &str) -> SavedServerOutcome {
    save_without_connecting_with(&CrateNfs, place, display_name).await
}

pub(crate) async fn save_without_connecting_with(
    backend: &dyn NfsBackend,
    place: KnownNfsPlace,
    display_name: &str,
) -> SavedServerOutcome {
    let place = KnownNfsPlace {
        export: cmdr_fs::volume::canonical_nfs_export(&place.export),
        start_folder: export_relative(place.start_folder.as_deref()),
        ..place
    };
    let volume_id = place.volume_id();
    let manager = crate::file_system::volume::manager::get_volume_manager();
    // Typed rather than a guess at the id's shape: only this backend's volume
    // has an identity to compare.
    let connected = manager
        .get(&volume_id)
        .and_then(|live| Some((backend.params_of(&*live)?, live)));
    let Some((live_params, live)) = connected else {
        // Not connected: the store is the only copy, and the next connect dials it.
        nfs_known_places::adopt_typed_name(&place, display_name);
        nfs_known_places::remember(place);
        return SavedServerOutcome::Saved;
    };
    let label = nfs_known_places::place_label_named(&server_name_for(display_name, &place), &place);
    let saved_start_folder = nfs_known_places::find(&volume_id).and_then(|saved| saved.start_folder);
    let connected_place = |live: Arc<dyn Volume>| ConnectedPlace {
        volume_id: volume_id.clone(),
        live,
        app_prefix: place.params().app_root(),
        saved_start_folder: saved_start_folder.clone(),
    };
    let edit = PlaceEdit {
        label: &label,
        // The export IS the root; it never moves in an edit.
        remote_root: "/",
        start_folder: place.start_folder.as_deref(),
    };

    if live_params.identity != place.identity() {
        let dialed = match backend.dial(&label, place.params(), CancellationToken::new()).await {
            Ok(dialed) => dialed,
            Err(e) => {
                log::info!(target: "volume", "an NFS place's new identity didn't connect: {:?}", failed(e));
                return SavedServerOutcome::Unreachable;
            }
        };
        if dialed.params.volume_id() != volume_id {
            // The server splits the saved export differently now, so this dial
            // reached another place.
            connect_wiring::let_go(dialed.volume).await;
            return SavedServerOutcome::Unreachable;
        }
        let fresh = Arc::clone(&dialed.volume);
        let successor_for = |name: &str, _: &Path| {
            backend
                .sharing_connection(&*fresh, name)
                .unwrap_or_else(|| Arc::clone(&fresh))
        };
        let checked = live_server_edit::check(
            &connected_place(Arc::clone(&dialed.volume)),
            &edit,
            successor_for,
            live_server_edit::CHECK_BUDGET,
        )
        .await;
        let accepted = match checked {
            Ok(accepted) => accepted,
            Err(refusal) => {
                connect_wiring::let_go(dialed.volume).await;
                return refusal;
            }
        };
        connect_wiring::install_retiring_incumbent(&volume_id, dialed.volume).await;
        nfs_known_places::adopt_typed_name(&place, display_name);
        nfs_known_places::remember(place);
        accepted.install(manager);
        log::info!(target: "volume", "{volume_id} redialed as its new identity");
        return SavedServerOutcome::Saved;
    }

    let successor_for = |name: &str, _: &Path| {
        backend
            .sharing_connection(&*live, name)
            .unwrap_or_else(|| Arc::clone(&live))
    };
    let accepted = match live_server_edit::check(
        &connected_place(Arc::clone(&live)),
        &edit,
        successor_for,
        live_server_edit::CHECK_BUDGET,
    )
    .await
    {
        Ok(accepted) => accepted,
        Err(refusal) => return refusal,
    };
    backend.set_auto_reconnect(&*live, place.auto_reconnect);
    // The live params are what the next redial and every `params()` reader
    // see, so the folder moves there too, ❌ not only in the store.
    backend.set_start_folder(&*live, place.start_folder.clone());
    nfs_known_places::adopt_typed_name(&place, display_name);
    nfs_known_places::remember(place);
    accepted.install(manager);
    SavedServerOutcome::Saved
}

/// Moves "reconnect automatically" on a saved place: the store, and a connected
/// volume's live copy. Answers whether a saved entry was there.
pub fn apply_auto_reconnect(volume_id: &str, on: bool) -> bool {
    if !nfs_known_places::set_auto_reconnect(volume_id, on) {
        return false;
    }
    let manager = crate::file_system::volume::manager::get_volume_manager();
    if let Some(live) = manager.get(volume_id) {
        CrateNfs.set_auto_reconnect(&*live, on);
    }
    true
}

/// Drops an NFS volume's connection and takes it out of the registry, answering
/// whether there was one.
pub async fn disconnect(volume_id: &str) -> bool {
    disconnect_with(&CrateNfs, volume_id).await
}

pub(crate) async fn disconnect_with(backend: &dyn NfsBackend, volume_id: &str) -> bool {
    let manager = crate::file_system::volume::manager::get_volume_manager();
    let Some(volume) = manager.get(volume_id) else {
        return false;
    };
    // Typed rather than a guess at the id's shape.
    if backend.params_of(&*volume).is_none() {
        return false;
    }
    manager.unregister(volume_id);
    // `on_unmount` closes the connection (and says `UMNT`, best effort): there's
    // no session to keep for anyone once the place is out of the registry.
    let _ = tokio::task::spawn_blocking(move || volume.on_unmount()).await;
    crate::volume_broadcast::emit_volumes_changed();
    log::info!(target: "volume", "disconnected NFS volume {volume_id}");
    true
}

#[cfg(test)]
#[path = "nfs_volume_wiring_test.rs"]
mod nfs_volume_wiring_test;
