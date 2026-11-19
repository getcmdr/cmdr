//! What this backend hands the shared tree walk: one walk for a stat, one
//! listing for a folder. The walk itself, the batch loop, and the conflict
//! matcher: `cmdr_fs::volume::scan_walk`.
//!
//! ❗ Both are the backend's OWN read path, ❌ never a listing-cache lookup:
//! with no watcher, `listing_watch_coverage` is `None` and a cached listing is
//! only as fresh as the last look.

use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::scan_walk::{ScanSource, Walking};
use std::path::Path;

use super::NfsVolume;

impl ScanSource for NfsVolume {
    fn scan_stat<'a>(&'a self, path: &'a AppPath) -> Walking<'a, FileEntry> {
        Box::pin(async move {
            let path_std = path.as_std_legacy();
            let path: &Path = &path_std;
            self.get_metadata_impl(path).await
        })
    }

    fn scan_list<'a>(&'a self, path: &'a AppPath) -> Walking<'a, Vec<FileEntry>> {
        Box::pin(async move {
            let path_std = path.as_std_legacy();
            let path: &Path = &path_std;
            self.list_directory_impl(path, None, None).await
        })
    }
}
