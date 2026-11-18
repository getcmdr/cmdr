//! Linux: `fdatasync` and `fsync` flush the drive's cache themselves (`one_sync.rs`).

pub(crate) use super::one_sync::{
    NEEDS_DEVICE_BARRIER, defers_data_sync, device_barrier, sync_dir_to_drive, sync_file_data, sync_to_drive,
    sync_volume,
};

/// A read-only open: `fsync` takes any fd, and a staged `0444` file refuses a write open.
pub(crate) fn open_to_sync(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// A read-only open, which `fsync` takes on a folder.
pub(crate) fn open_dir_to_sync(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}
