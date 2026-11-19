//! What this backend hands the shared tree walk: one stat, one listing. The
//! walk itself, the batch loop, the conflict matcher, and the reasoning behind
//! all three: `cmdr_fs::volume::scan_walk`.
//!
//! ❗ Both methods are the backend's OWN read path, ❌ never a listing-cache
//! lookup. This backend has no watcher, so its `listing_watch_coverage` is
//! `None` and a cached listing is only as fresh as the last time somebody
//! looked. SMB's scan may consult the cache because its watcher backs the claim;
//! borrowing that shortcut here is how a pre-flight conflict scan misses a file
//! and a copy overwrites it.

use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::scan_walk::{ScanSource, Walking};

use super::SftpVolume;

impl ScanSource for SftpVolume {
    fn scan_stat<'a>(&'a self, path: &'a AppPath) -> Walking<'a, FileEntry> {
        Box::pin(async move { self.get_metadata_impl(path).await })
    }

    fn scan_list<'a>(&'a self, path: &'a AppPath) -> Walking<'a, Vec<FileEntry>> {
        Box::pin(async move { self.list_directory_impl(path, None, None).await })
    }
}
