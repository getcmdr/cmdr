//! Change intake: what the host heard about a listed directory, and the data
//! it brings for a listing's rows.

use std::path::Path;

use crate::cached_listing::from_host_seam;

use cmdr_fs::entry::TagRef;
use cmdr_fs::volume::DirectoryChange;

use crate::ListingLookupError;
use crate::index_sizes::IndexSizes;
use crate::listing_id::ListingId;

use super::Listings;

/// What a deferred pass learned about some rows, keyed by path: the facts the
/// bulk listing leaves out because each costs a syscall or a framework call per
/// row, so the host reads them for the visible rows only.
#[derive(Debug, Clone)]
pub enum RowFacts {
    /// Each path's Finder tags, replacing the row's (an empty list clears them).
    Tags(Vec<(String, Vec<TagRef>)>),
    /// Whether macOS presents each path as one item. Rows the package list
    /// already answers are left alone; a change re-places the row, since a
    /// package has an extension.
    Packages(Vec<(String, bool)>),
}

impl Listings {
    /// Patches every pane listing `parent_path` on `volume_id` with `change`.
    /// Panes only: the host applies the change to its other subsystems (an
    /// index, file badges) BEFORE calling this, so the enrich here sees them.
    pub fn directory_changed(&self, volume_id: &str, parent_path: &Path, change: DirectoryChange) {
        self.inner
            .notify_directory_changed(volume_id, &from_host_seam(parent_path), change);
    }

    /// Writes what a deferred, visible-row pass read for `id`'s rows, by path,
    /// and tells the pane: Finder tags, or which dotted folders macOS calls
    /// packages. See [`RowFacts`].
    pub fn apply_row_facts(&self, id: &ListingId, facts: RowFacts) {
        match facts {
            RowFacts::Tags(updates) => self.inner.apply_tags_to_listing(id.as_str(), updates),
            RowFacts::Packages(updates) => self.inner.apply_packages_to_listing(id.as_str(), updates),
        }
    }

    /// Writes folder sizes onto `id`'s rows by path, as data: no name or path
    /// can change under the cache lock. Keeps the listing's row and path maps,
    /// doesn't re-sort, and doesn't count as the pane using the listing.
    /// Returns how many of `sizes` found a row.
    pub fn update_index_sizes(
        &self,
        id: &ListingId,
        sizes: &[(String, IndexSizes)],
    ) -> Result<usize, ListingLookupError> {
        self.inner.update_index_sizes(id.as_str(), sizes)
    }

    /// Asks the host's index again about every folder of `id`, with no engine
    /// lock held while it answers.
    pub fn refresh_index_sizes(&self, id: &ListingId) {
        self.inner.enrich_index_sizes(id.as_str());
    }

    /// Re-reads every listing on `volume_id` in the spelling its volume serves
    /// now (an SMB share whose backend changed).
    pub fn respell_on_volume(&self, volume_id: &str) {
        self.inner.respell_listings_on_volume(volume_id);
    }

    /// Fully re-reads every listing at or inside `archive_path` on `volume_id`.
    pub async fn refresh_archive_listings(&self, volume_id: &str, archive_path: &Path) {
        self.inner
            .refresh_archive_listings(volume_id, &from_host_seam(archive_path))
            .await;
    }
}
