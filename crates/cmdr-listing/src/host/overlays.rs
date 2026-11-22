//! Rows a PANE shows that no volume holds.
//!
//! A [`ListingOverlay`] contributes extra [`FileEntry`]s to one directory's
//! listing, folded in by the listing pipeline after the volume's own entries
//! arrive. Cmdr's contributors are the virtual `.git` portal, which puts the six
//! category rows (`branches/`, `tags/`, …) into a repo's `.git/` listing, and
//! restic, which puts a `Browse snapshots` row into a backup repo's folder.
//! Every real entry beside them stays the volume's, editable and deletable as
//! always.
//!
//! ❌ **Never move this into a `Volume` impl**: a contributed row must reach a
//! pane and nothing else, which is also why a decorated listing never answers
//! the fresh-listing oracle. Why: `host/DETAILS.md` § "Overlays".
//!
//! The host hands its contributors to the engine's builder, in order
//! (`ListingsBuilder::overlay`), and the read pipeline (`pipeline.rs`) folds
//! over whatever the engine holds, between the enrich and the sort, on every
//! read that builds or replaces a listing.

use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::Volume;

/// A source of extra rows for one kind of directory.
pub trait ListingOverlay: Send + Sync + 'static {
    /// The contributor's stable name (`"git-portal"`), the key the slot dedupes
    /// on.
    fn id(&self) -> &'static str;

    /// Whether this overlay has anything to say about `path` on `volume`, whose
    /// listing holds `entries` (the volume's own, enriched, before any
    /// contributor's rows).
    ///
    /// ❗ Cheap and zero-I/O: this runs on EVERY directory listing in the app,
    /// so it has to be a look at the path, the volume's capability flags, and
    /// the entries already in hand. A contributor that scans `entries` caps
    /// itself (answer `false` past a small count), so a 100k-entry folder costs
    /// a length check. The expensive half is
    /// [`extra_entries`](Self::extra_entries), which only runs when this
    /// answered `true`.
    fn applies_to(&self, volume: &dyn Volume, path: &AppPath, entries: &[FileEntry]) -> bool;

    /// The rows to fold in. Runs on the blocking pool, so it may open a
    /// repository or hit the disk. `volume_id` is the id the listing was read
    /// under, the same one a later `resolve` of a path below sees, so a
    /// contributor can key what it learns here the way its route looks it up.
    fn extra_entries(&self, volume_id: &str, volume: &dyn Volume, path: &AppPath) -> Vec<FileEntry>;
}
