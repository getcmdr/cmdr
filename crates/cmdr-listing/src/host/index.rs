//! What the drive index knows about the rows a listing holds.

use cmdr_fs::app_path::AppPath;

use cmdr_fs::entry::FileEntry;

/// Answers the engine's questions of the drive index.
///
/// The index's own change intake (`apply_directory_change`) is deliberately
/// not here: it's a different subsystem hearing the same change, so the app
/// composes it ahead of the panes rather than the engine calling it.
pub trait ListingIndex: Send + Sync {
    /// Fills in what the index knows about `entries`, all from one directory on
    /// `volume_id`, in one call.
    ///
    /// ❗ The contract the engine relies on: it writes ONLY the seven
    /// `recursive_*` fields, and only on directories that aren't symlinks. So
    /// the engine can enrich a clone of just those rows and write them back.
    fn enrich(&self, volume_id: &str, entries: &mut [FileEntry]);

    /// A pane just listed `path` on `volume_id`; the index may compare the listing
    /// with what it holds and schedule a fix.
    fn verify_directory(&self, volume_id: &str, path: &AppPath);
}

/// No index: nothing enriched, nothing verified.
pub struct NoIndex;

impl ListingIndex for NoIndex {
    fn enrich(&self, _volume_id: &str, _entries: &mut [FileEntry]) {}
    fn verify_directory(&self, _volume_id: &str, _path: &AppPath) {}
}
