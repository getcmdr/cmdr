//! Windows: `FlushFileBuffers` flushes the drive's cache itself (`one_sync.rs`).

pub(crate) use super::one_sync::{
    NEEDS_DEVICE_BARRIER, defers_data_sync, device_barrier, sync_dir_to_drive, sync_file_data, sync_to_drive,
    sync_volume,
};

/// A write open: `FlushFileBuffers` refuses a handle without write access (verified on Windows 10
/// 22H2, `cargo test -p cmdr-fs` on the builder, 2026-11-18: `Access is denied`). A file wearing
/// the read-only attribute refuses this open too; no Windows arm writes that attribute onto a
/// staged file yet (the windows-port plan's task 36 decides the order when one does).
pub(crate) fn open_to_sync(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::options().write(true).open(path)
}

/// A folder opens only with backup semantics (a plain open answers `Access is denied`), and
/// `FlushFileBuffers` wants write access on it as on a file. Backup semantics bypass the access
/// check only for a process that ENABLED `SeBackupPrivilege`, which Cmdr never does.
pub(crate) fn open_dir_to_sync(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

    std::fs::File::options()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}
