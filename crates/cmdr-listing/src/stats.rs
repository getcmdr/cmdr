//! What a pane's status line counts: files, folders, and sizes, in total and for
//! the selection, over the rows the pane shows.

use serde::{Deserialize, Serialize};

use crate::Inner;
use crate::ListingLookupError;
use crate::visible_rows::VisibleRows;

/// Statistics about a directory listing.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListingStats {
    /// Not including directories.
    pub total_files: usize,
    /// How many directories the pane shows.
    pub total_dirs: usize,
    /// Total logical size in bytes (files + directory recursive sizes).
    pub total_size: u64,
    /// Total physical (on-disk) size in bytes. Mirrors `total_size` but uses `physical_size` /
    /// `recursive_physical_size`.
    pub total_physical_size: u64,
    /// Present only if `selected_indices` was provided.
    pub selected_files: Option<usize>,
    /// Present only if `selected_indices` was provided.
    pub selected_dirs: Option<usize>,
    /// Total logical size of selected entries in bytes. Present only if `selected_indices` was
    /// provided.
    pub selected_size: Option<u64>,
    /// Total physical size of selected entries in bytes. Present only if `selected_indices` was
    /// provided.
    pub selected_physical_size: Option<u64>,
}

impl Inner {
    /// Gets statistics about a cached listing.
    ///
    /// Returns total file/dir counts and sizes. If `selected_indices` is provided,
    /// also returns statistics for the selected items.
    pub(crate) fn get_listing_stats(
        &self,
        listing_id: &str,
        include_hidden: bool,
        selected_indices: Option<&[usize]>,
    ) -> Result<ListingStats, ListingLookupError> {
        self.with_listing(listing_id, |listing| {
            listing_stats(listing.rows(include_hidden), selected_indices)
        })
    }
}

/// An entry's content and on-disk sizes as the sums count them: a folder's recursive
/// sizes, a file's own. An entry with no on-disk size (every file on Windows, a network
/// share's files) counts its content size there, the fallback its row shows
/// (`getDisplaySize`). Counting nothing left a physical sum covering only some entries,
/// which the "smart" size then picked over the whole content sum: "16 bytes of 7 bytes
/// (229%) selected".
fn sizes_of(entry: &cmdr_fs::entry::FileEntry) -> (Option<u64>, Option<u64>) {
    let (logical, physical) = if entry.is_directory {
        (entry.recursive_size, entry.recursive_physical_size)
    } else {
        (entry.size, entry.physical_size)
    };
    (logical, physical.or(logical))
}

/// The counting half of `get_listing_stats`, over one snapshot of the pane's rows.
fn listing_stats(visible: VisibleRows<'_>, selected_indices: Option<&[usize]>) -> ListingStats {
    // Calculate totals
    let mut total_files: usize = 0;
    let mut total_dirs: usize = 0;
    let mut total_size: u64 = 0;
    let mut total_physical_size: u64 = 0;

    for entry in visible.iter() {
        if entry.is_directory {
            total_dirs += 1;
        } else {
            total_files += 1;
        }
        let (logical, physical) = sizes_of(entry);
        total_size += logical.unwrap_or(0);
        total_physical_size += physical.unwrap_or(0);
    }

    // Calculate selection stats if indices provided
    let (selected_files, selected_dirs, selected_size, selected_physical_size) = if let Some(indices) = selected_indices
    {
        let mut sel_files: usize = 0;
        let mut sel_dirs: usize = 0;
        let mut sel_size: u64 = 0;
        let mut sel_physical_size: u64 = 0;

        for &idx in indices {
            if let Some(entry) = visible.get(idx) {
                if entry.is_directory {
                    sel_dirs += 1;
                } else {
                    sel_files += 1;
                }
                let (logical, physical) = sizes_of(entry);
                sel_size += logical.unwrap_or(0);
                sel_physical_size += physical.unwrap_or(0);
            }
        }

        (Some(sel_files), Some(sel_dirs), Some(sel_size), Some(sel_physical_size))
    } else {
        (None, None, None, None)
    };

    ListingStats {
        total_files,
        total_dirs,
        total_size,
        total_physical_size,
        selected_files,
        selected_dirs,
        selected_size,
        selected_physical_size,
    }
}
