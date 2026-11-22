//! Whether a write into a folder would be taken (`Volume::write_access_at`), the Windows twin of
//! the POSIX `statvfs` plus `access(W_OK)` pair: the volume's read-only flag, then an open of the
//! folder asking for `FILE_ADD_FILE` (the right to make an entry in it), which runs the real
//! access check (the DACL) and writes nothing. What the two answers mean together:
//! `super::super::write_verdict`.
//!
//! Backup semantics only let a folder open at all; they bypass the check only for a process that
//! ENABLED `SeBackupPrivilege`, which Cmdr never does. A folder's read-only attribute isn't
//! consulted: it's Explorer's customization marker, and NTFS lets anyone with the right make
//! entries in such a folder.

use std::fs::File;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::Path;

use cmdr_fs::volume::WriteAccess;

use super::super::write_verdict::write_verdict;

/// `FILE_READ_ONLY_VOLUME` in `GetVolumeInformationByHandleW`'s flags.
const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;

/// The verdict for the nearest folder at or above `path` (the OS's path) that exists, since a
/// copy creates the rest. `Unknown` when none can be found or opened.
pub(crate) fn write_access_at(path: &Path) -> WriteAccess {
    let Some(folder) = path.ancestors().find(|ancestor| ancestor.is_dir()) else {
        return WriteAccess::Unknown;
    };
    let Ok(handle) = open_folder(folder, 0) else {
        return WriteAccess::Unknown;
    };
    let read_only_volume = volume_flags(&handle).is_some_and(|flags| flags & FILE_READ_ONLY_VOLUME != 0);
    drop(handle);
    use windows_sys::Win32::Storage::FileSystem::FILE_ADD_FILE;
    let open = open_folder(folder, FILE_ADD_FILE)
        .map(drop)
        .map_err(|e| e.raw_os_error());
    write_verdict(read_only_volume, open)
}

/// `folder` opened asking for `access` (0: attributes only), every share mode, so another
/// program's handle never makes the probe refuse.
fn open_folder(folder: &Path, access: u32) -> std::io::Result<File> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    std::fs::OpenOptions::new()
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(folder)
}

/// The filesystem flags of the volume `handle` sits on, or `None` when Windows can't say.
fn volume_flags(handle: &File) -> Option<u32> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;
    let mut flags = 0u32;
    // SAFETY: `handle` is open for the call; every out pointer but `flags` is null with a zero
    // length (not wanted), and `flags` is a live `u32`.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut flags,
            std::ptr::null_mut(),
            0,
        )
    };
    (ok != 0).then_some(flags)
}
