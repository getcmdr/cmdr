//! The write engine's own-write announces, answered by the downloads watcher's
//! ignore set, so a file Cmdr lands in `~/Downloads` never reads as a new
//! download, and by the listing watches at or below the path whose OS handle
//! pins the folders above it (Windows'), which let go first, since NTFS refuses
//! to rename a folder above an open one (#1340, `file_system/watcher/release.rs`).

use std::path::Path;

use cmdr_operations::host::OwnWrites;

/// `downloads::note_pending_write_for_cmdr`, which no-ops while the watcher
/// sleeps and for a path outside the Downloads root.
pub(super) struct AppOwnWrites;

impl OwnWrites for AppOwnWrites {
    fn announce(&self, path: &Path) {
        crate::downloads::note_pending_write_for_cmdr(path);
        // A backend whose watch pins nothing (FSEvents, inotify) finds no watch to release.
        crate::file_system::watcher::release_watches_at_or_below(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downloads::own_write_recorder::{Announced, record_under};
    use crate::test_support::TestDir;

    /// An announce reaches the downloads hook, path for path, while nothing
    /// exists there yet.
    #[test]
    fn an_announce_reaches_the_downloads_hook() {
        let dir = TestDir::new("operations_host_own_writes");
        let recording = record_under(&dir);
        let path = dir.join("incoming.bin");

        AppOwnWrites.announce(&path);

        assert_eq!(recording.announced(), vec![Announced { path, existed: false }]);
    }
}
