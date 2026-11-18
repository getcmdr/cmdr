//! The browser build has no local disk; std's syncs answer for it (`one_sync.rs`).

pub(crate) use super::one_sync::{
    NEEDS_DEVICE_BARRIER, defers_data_sync, device_barrier, sync_dir_to_drive, sync_file_data, sync_to_drive,
    sync_volume,
};

/// A read-only open, as on Linux.
pub(crate) fn open_to_sync(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// A read-only open, as on Linux.
pub(crate) fn open_dir_to_sync(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}
