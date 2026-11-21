//! Windows: NTFS has no POSIX mode and no symlink a process may always create, so
//! those refuse as unsupported (windows-port plan, tasks 35 and 36). The write
//! probe, space, and the folder volume are real, the last two through
//! `cmdr-local`'s Windows arms.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;

use super::super::WriteProbe;

/// The read-only attribute is the only mode-like fact a Windows file carries.
pub(crate) fn mode_of(permissions: &std::fs::Permissions) -> u32 {
    if permissions.readonly() { 0o444 } else { 0o644 }
}

/// A POSIX mode has no Windows meaning (a folder's read-only attribute is
/// Explorer's customization marker, not a write lock), so this refuses.
pub(crate) fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

pub(crate) fn create_dir_with_mode(dir: &Path, _mode: u32) -> std::io::Result<()> {
    std::fs::create_dir(dir)
}

pub(crate) fn with_mode(options: &mut std::fs::OpenOptions, _mode: u32) -> &mut std::fs::OpenOptions {
    options
}

/// Windows tells file links from folder links and needs a privilege (or
/// Developer Mode) to create either; until task 35's link support lands, a copied
/// link is refused rather than turned into something else.
pub(crate) fn symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// `access(W_OK)`'s Windows twin: open `path` asking for `FILE_WRITE_DATA` (on a
/// folder the same bit is `FILE_ADD_FILE`, the right to create an entry in it), and
/// close it again. The open runs the real access check (the DACL, and the
/// read-only attribute on a file) and writes nothing. Backup semantics only let a
/// folder open at all; they bypass the check only for a process that ENABLED
/// `SeBackupPrivilege`, which Cmdr never does.
///
/// A sharing violation (another program opened it without sharing writes) says
/// nothing about permission, so it's `Unknown` and the write itself decides.
pub(crate) fn write_probe(path: &Path) -> WriteProbe {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_DATA,
    };

    let opened = std::fs::OpenOptions::new()
        .access_mode(FILE_WRITE_DATA)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path);
    match opened {
        Ok(_handle) => WriteProbe::Allowed,
        Err(e) => match e.raw_os_error() {
            Some(code) if code == ERROR_SHARING_VIOLATION as i32 => WriteProbe::Unknown,
            Some(code) => WriteProbe::Refused(code),
            // std refuses a path it can't hand to Win32 (an interior NUL) before any call.
            None => WriteProbe::Unrepresentable,
        },
    }
}

/// `GetDiskFreeSpaceExW`'s figure for this user (quotas included), which is cheap.
pub(crate) fn free_bytes(path: &Path) -> Option<u64> {
    cmdr_local::space::get_space_info_for_path(path).ok()?.available_bytes()
}

/// No purgeable-space figure; `free_bytes` is the whole answer.
pub(crate) fn important_free_bytes(_path: &Path) -> Option<u64> {
    None
}

/// The read-only attribute on a FILE is Windows' Locked: Explorer's "Read-only" box,
/// which the delete and Overwrite paths refuse as `FileLocked` ([`locked_against_removal`]).
/// ❗ A folder's read-only bit isn't a lock: Explorer sets it to mark a customized
/// folder (`desktop.ini`), and nothing honors it as one. Never follows a link.
pub(crate) fn is_locked_item(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().readonly())
}

/// A read-only file: `std::fs::remove_file` deletes it anyway (it clears the
/// attribute on the way, verified on Windows 10 22H2 with Rust 1.100, a permanent
/// delete in the running app, 2026-11-21), and an Overwrite's rename carries it
/// aside, so only an up-front ask keeps the lock.
pub(crate) fn locked_against_removal(path: &Path) -> bool {
    is_locked_item(path)
}

pub(crate) fn folder_volume(name: &str, root: PathBuf, host: VolumeHost) -> Arc<dyn Volume> {
    Arc::new(cmdr_local::LocalVolume::local_folder(name, root, host))
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
