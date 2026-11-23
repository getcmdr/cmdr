//! The drive index's folder sizes on a listing's rows, refreshed when the index
//! learns something new, with no engine lock held while the index answers.

use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::RwLockIgnorePoison;

use super::Inner;
use super::cached_listing::CachedListing;
use super::diff::DiffChange;
use super::operations::ListingLookupError;
use super::read_generation::ReadGeneration;

/// The seven `recursive_*` fields the drive index fills in on a folder row: data,
/// so a writer can't touch a name or a path under the cache lock. Each field
/// means what the [`FileEntry`] field of the same name means.
// DEFAULT-OK: every field `None` is "the index knows nothing about this folder".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndexSizes {
    /// The folder's logical size in bytes, everything below it counted.
    pub recursive_size: Option<u64>,
    /// The folder's on-disk size in bytes, everything below it counted.
    pub recursive_physical_size: Option<u64>,
    /// How many files sit anywhere below the folder.
    pub recursive_file_count: Option<u64>,
    /// How many folders sit anywhere below the folder.
    pub recursive_dir_count: Option<u64>,
    /// Whether a symlink sits anywhere below the folder.
    pub recursive_has_symlinks: Option<bool>,
    /// Whether the index has walked everything below the folder.
    pub recursive_size_complete: Option<bool>,
    /// Whether the numbers are accurate but stale: nothing below the folder
    /// was re-listed since the index lost track of changes there.
    pub recursive_size_stale: Option<bool>,
}

impl IndexSizes {
    /// What `entry` holds now.
    pub fn of(entry: &FileEntry) -> Self {
        Self {
            recursive_size: entry.recursive_size,
            recursive_physical_size: entry.recursive_physical_size,
            recursive_file_count: entry.recursive_file_count,
            recursive_dir_count: entry.recursive_dir_count,
            recursive_has_symlinks: entry.recursive_has_symlinks,
            recursive_size_complete: entry.recursive_size_complete,
            recursive_size_stale: entry.recursive_size_stale,
        }
    }

    /// Writes these sizes onto `entry`, leaving every other field alone.
    pub(crate) fn apply_to(self, entry: &mut FileEntry) {
        entry.recursive_size = self.recursive_size;
        entry.recursive_physical_size = self.recursive_physical_size;
        entry.recursive_file_count = self.recursive_file_count;
        entry.recursive_dir_count = self.recursive_dir_count;
        entry.recursive_has_symlinks = self.recursive_has_symlinks;
        entry.recursive_size_complete = self.recursive_size_complete;
        entry.recursive_size_stale = self.recursive_size_stale;
    }
}

/// The only rows the index's enrich writes (`host/index.rs`).
fn holds_index_sizes(entry: &FileEntry) -> bool {
    entry.is_directory && !entry.is_symlink
}

/// What the index answered about a listing's folders, asked with no engine lock
/// held, and the PUBLISHED read generation the answer is good for.
pub(crate) struct AskedSizes {
    generation: ReadGeneration,
    folders: Vec<FileEntry>,
}

impl AskedSizes {
    /// Writes the answer onto `listing` by path, unless a full re-read
    /// published in between, and returns the entry index of every row whose
    /// sizes moved.
    ///
    /// That re-read stored rows of its own, enriched later than these were (the
    /// replace step asks again once its rows land, `reread.rs`), so these answers
    /// are the stale ones and are dropped. The check is against the PUBLISHED
    /// generation, never the last one handed out: an in-flight re-read that then
    /// fails or is superseded must not cost these sizes with nothing replacing
    /// them. A `Modified` patch in between takes no generation, so it can get
    /// sizes one index event older written over its own enrich; the next event
    /// corrects it, the same window two events always had.
    ///
    /// By path is order-independent, so a re-sort in between is harmless; a
    /// folder added in between was enriched by its own insert; one removed or
    /// renamed is skipped. Writes keep both maps, don't re-sort, and don't bump
    /// `last_accessed_ms`: an index event isn't the user.
    pub(crate) fn write_to(self, listing: &mut CachedListing) -> Vec<usize> {
        if listing.published_generation != self.generation {
            return Vec::new();
        }
        let paths: Vec<String> = self.folders.iter().map(|folder| folder.path.clone()).collect();
        listing.update_index_sizes_by_path(&paths, |position, entry| {
            let sizes = IndexSizes::of(&self.folders[position]);
            // A row that stopped being a folder in between (a patch) gets none.
            if !holds_index_sizes(entry) || IndexSizes::of(entry) == sizes {
                return false;
            }
            sizes.apply_to(entry);
            true
        })
    }
}

impl Inner {
    /// Re-enriches a cached listing's folders with what the index knows now, for
    /// an index event (`DirsUpdated`), so a later `get_listing_stats` sees fresh
    /// sizes.
    #[cfg(test)]
    pub(crate) fn refresh_listing_index_sizes(&self, listing_id: &str) -> Result<(), String> {
        self.enrich_index_sizes(listing_id);
        Ok(())
    }

    /// Asks the index about `listing_id`'s folders with NO engine lock held.
    /// `None` when the listing is gone or holds no folder.
    ///
    /// Under the cache write lock this was two SQLite queries per open pane per
    /// index event, and every pane read and IPC accessor waited behind them.
    pub(crate) fn ask_index_sizes(&self, listing_id: &str) -> Option<AskedSizes> {
        let (volume_id, generation, mut folders) = {
            let cache = self.cache.read_ignore_poison();
            let listing = cache.get(listing_id)?;
            let folders: Vec<FileEntry> = listing
                .entries()
                .iter()
                .filter(|entry| holds_index_sizes(entry))
                .cloned()
                .collect();
            (listing.volume_id.clone(), listing.published_generation, folders)
        };
        if folders.is_empty() {
            return None;
        }
        self.index.enrich(&volume_id, &mut folders);
        Some(AskedSizes { generation, folders })
    }

    /// Asks the index about `listing_id`'s folders with no engine lock held,
    /// writes the answers back ([`AskedSizes::write_to`]), and publishes every
    /// row whose sizes moved as a modify, under the same write lock.
    ///
    /// The publish is what keeps the pane on the cache's numbers: the app's
    /// index-size worker sends only what differs from the cache, so a size
    /// written here silently (say the replace step's, newer than the pipeline's
    /// that the re-read's diff carried) would never reach the pane.
    pub(crate) fn enrich_index_sizes(&self, listing_id: &str) {
        let Some(asked) = self.ask_index_sizes(listing_id) else {
            return;
        };
        // Resolved before the lock: it asks the host.
        let visibility = self.scratch_visibility();
        let mut cache = self.write_cache();
        let Some((listing, publish)) = cache.get_mut(listing_id) else {
            return;
        };
        listing.reconcile_scratch(listing_id, visibility, &publish);
        let moved = asked.write_to(listing);
        // A size moves no row and hides none, so a row answers both sides.
        let pane = listing.pane_rows();
        let changes = moved
            .into_iter()
            .filter_map(|index| {
                let row = pane.row_of_entry(index)?;
                Some(DiffChange::modified(listing.entries()[index].clone(), row))
            })
            .collect();
        drop(pane);
        listing.publish_changes(&publish, listing_id, changes);
    }

    /// Writes `sizes` onto `listing_id`'s rows by path, keeping both maps, with
    /// no re-sort and no `last_accessed_ms` bump. Returns how many found a row.
    pub(crate) fn update_index_sizes(
        &self,
        listing_id: &str,
        sizes: &[(String, IndexSizes)],
    ) -> Result<usize, ListingLookupError> {
        let paths: Vec<String> = sizes.iter().map(|(path, _)| path.clone()).collect();
        let mut cache = self.cache.write_ignore_poison();
        let listing = cache
            .get_mut(listing_id)
            .ok_or_else(|| ListingLookupError::gone(listing_id))?;
        let mut written = 0;
        listing.update_index_sizes_by_path(&paths, |position, entry| {
            sizes[position].1.apply_to(entry);
            written += 1;
            true
        });
        Ok(written)
    }
}
