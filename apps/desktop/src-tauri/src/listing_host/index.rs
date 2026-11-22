//! The drive index, answering what the listing engine asks of it.

use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;

use cmdr_listing::host::ListingIndex;

/// Answers from `index_host::index()`, resolved per call: this adapter is built
/// before `index_host::install` runs, and capturing the index then would win
/// its slot with an unconfigured one.
pub(crate) struct AppListingIndex;

impl ListingIndex for AppListingIndex {
    fn enrich(&self, volume_id: &str, entries: &mut [FileEntry]) {
        crate::index_host::index().enrich(volume_id, entries);
    }

    fn verify_directory(&self, volume_id: &str, path: &AppPath) {
        crate::index_host::index().verify_directory(volume_id, path.as_str());
    }
}
