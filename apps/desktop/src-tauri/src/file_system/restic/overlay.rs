//! The `Browse snapshots` row that joins a restic repo's real listing.
//!
//! Everything at and below `<repo>/Browse snapshots` is a routed volume
//! (`cmdr_restic::ResticVolume`). The repo folder itself can't be: its real
//! files (`config`, `data/`, …) must stay ordinary files, listable, copyable,
//! and deletable. So the row reaches the pane as a [`ListingOverlay`]
//! contribution instead, and no walker ever sees it.
//! `volume/DETAILS.md` § "Architecture".

use cmdr_fs::app_path::AppPath;
use std::sync::Arc;

use crate::file_system::volume::Volume;
use cmdr_fs::entry::FileEntry;
use cmdr_listing::host::ListingOverlay;
use cmdr_restic::is_restic_repo;

use super::wiring;

/// Contributes the `Browse snapshots` row to a restic repo's own listing.
pub struct ResticOverlay;

/// The repo row's overlay, for the listing host to install.
pub fn overlay() -> Arc<dyn ListingOverlay> {
    Arc::new(ResticOverlay)
}

impl ListingOverlay for ResticOverlay {
    fn id(&self) -> &'static str {
        "restic-snapshots"
    }

    /// The listing's own entries look like a restic repo, on a volume a route
    /// didn't mint.
    ///
    /// ❗ Zero I/O and O(1) for a big folder: a listing past
    /// [`wiring::MAX_REPO_ROOT_ENTRIES`] answers on its length alone. Any
    /// unrouted volume qualifies (local, SMB, SFTP, a phone), since the repo is
    /// read through `Volume`; ❌ never a routed one, so a repo backed up inside a
    /// snapshot or zipped in an archive doesn't nest a second view.
    fn applies_to(&self, volume: &dyn Volume, _path: &AppPath, entries: &[FileEntry]) -> bool {
        !volume.routes_over_a_parent() && entries.len() <= wiring::MAX_REPO_ROOT_ENTRIES && is_restic_repo(entries)
    }

    /// The one row. Building it also confirms the repo in the hub's memo, keyed
    /// by the same `(volume id, folder)` the route looks up, so entering the
    /// row costs no confirmation listing.
    fn extra_entries(&self, volume_id: &str, _volume: &dyn Volume, path: &AppPath) -> Vec<FileEntry> {
        vec![wiring::repos().overlay_row_at(volume_id, path)]
    }
}
