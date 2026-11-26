//! Keeping the Servers list (`smb://` on the `network` volume) current with no poll.
//!
//! Its rows are a fold over memory (`servers_listing.rs`), so a change to any input
//! (a host found, lost, or resolved, a `volumes-changed`, a share list's account) asks
//! for ONE re-list once the burst goes quiet, handed to the panes as a whole
//! (`DirectoryChange::Replaced`): the engine diffs it by path and patches.
//!
//! ❗ Nothing happens while no pane shows `smb://`: a request with no listing open costs
//! a lookup and schedules nothing, and a quiet burst checks again before it builds rows.
//! A discovery storm is one re-list per quiet window, ❌ never one per event.

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use cmdr_fs::volume::DirectoryChange;

use crate::network::share_list_path::NetworkPath;
use crate::network::share_list_volume::{NETWORK_VOLUME_ID, SmbServerVolume};

/// How long the inputs stay quiet before the list re-lists: a browse resolving a dozen
/// hosts, or a mount that changes the volume list twice, is one re-list.
pub(crate) const QUIET: Duration = Duration::from_millis(150);

/// The Servers list's listings, as the re-list sees them. The app's answer is
/// [`AppServersList`]; a test hands in its own.
pub(crate) trait ServersList: Send + Sync {
    /// Whether a pane shows the Servers list.
    fn is_shown(&self) -> bool;

    /// Builds the rows afresh and hands them to the panes showing them.
    fn relist(&self);
}

/// The debounce: each request bumps the generation, and only the request still newest
/// once [`QUIET`] has passed re-lists.
pub(crate) struct Relist {
    generation: AtomicU64,
    list: Arc<dyn ServersList>,
}

impl Relist {
    pub(crate) fn new(list: Arc<dyn ServersList>) -> Arc<Self> {
        Arc::new(Self {
            generation: AtomicU64::new(0),
            list,
        })
    }

    /// Asks for a re-list. `None` when no pane shows the list (nothing to spawn);
    /// otherwise the wait to spawn, which re-lists only if no newer request came in.
    pub(crate) fn request(self: &Arc<Self>) -> Option<impl Future<Output = ()> + Send + 'static> {
        if !self.list.is_shown() {
            return None;
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let relist = Arc::clone(self);
        Some(async move {
            tokio::time::sleep(QUIET).await;
            if relist.generation.load(Ordering::SeqCst) == generation && relist.list.is_shown() {
                relist.list.relist();
            }
        })
    }
}

/// The Servers list on a registered volume: the app's `network`, or a test's own id.
pub(crate) struct AppServersList {
    pub volume_id: String,
}

impl AppServersList {
    /// Every spelling of `smb://` a listing on the volume is open at, as the listing
    /// host seam hands it back: what a change is reported under.
    fn shown_at(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = crate::listing_host::listings()
            .open_listing_paths(&self.volume_id)
            .into_iter()
            .filter(|path| {
                path.to_str()
                    .is_some_and(|text| NetworkPath::of(text) == NetworkPath::Hub)
            })
            .collect();
        paths.sort();
        paths.dedup();
        paths
    }
}

impl ServersList for AppServersList {
    fn is_shown(&self) -> bool {
        !self.shown_at().is_empty()
    }

    fn relist(&self) {
        let Some(volume) = crate::file_system::volume::manager::get_volume_manager().get(&self.volume_id) else {
            return;
        };
        let Some(servers) = volume.as_any().downcast_ref::<SmbServerVolume>() else {
            return;
        };
        let entries = servers.server_entries();
        for path in self.shown_at() {
            crate::listing_host::directory_changed(&self.volume_id, &path, DirectoryChange::Replaced(entries.clone()));
        }
    }
}

/// The app's one debounce, over the `network` volume.
fn app_relist() -> &'static Arc<Relist> {
    static RELIST: OnceLock<Arc<Relist>> = OnceLock::new();
    RELIST.get_or_init(|| {
        Relist::new(Arc::new(AppServersList {
            volume_id: NETWORK_VOLUME_ID.to_string(),
        }))
    })
}

/// Asks the app's Servers list to re-list once its inputs go quiet. Any thread.
pub(crate) fn request() {
    if let Some(wait) = app_relist().request() {
        tauri::async_runtime::spawn(wait);
    }
}

/// Subscribes the re-list to every event that changes a Servers row's input: discovery
/// (found, lost, resolved) and the volume list, which every mount, unmount, saved-store
/// write, and remembered mounted server republishes (`volume_broadcast.rs`).
///
/// ❗ By the typed event's NAME with an untyped listener, ❌ never `Event::listen_any`:
/// that one deserializes every payload, and `VolumesChanged` doesn't round-trip (a field
/// skipped when serialized is required when read back), so it panicked on the runtime's
/// worker and took the volume list down with it (Linux E2E, 2026-11-26). The re-list
/// needs only "it changed".
pub(crate) fn subscribe(app: &tauri::AppHandle) {
    use tauri::Listener;
    use tauri_specta::Event;

    use crate::network::events::{NetworkHostFound, NetworkHostLost, NetworkHostResolved};
    use crate::volume_broadcast::VolumesChanged;

    for name in [
        NetworkHostFound::NAME,
        NetworkHostLost::NAME,
        NetworkHostResolved::NAME,
        VolumesChanged::NAME,
    ] {
        app.listen_any(name, |_| request());
    }
}

#[cfg(test)]
#[path = "servers_relist_test.rs"]
mod tests;
