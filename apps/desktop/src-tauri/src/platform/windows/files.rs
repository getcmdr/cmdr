//! Two file facts several Windows arms ask: where the system folder is, and whether a program
//! path names something that starts (a file, or an app execution alias).

use std::path::{Path, PathBuf};

/// The system folder (`C:\Windows\System32`), as Windows reports it.
pub(crate) fn system_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let mut buf = [0u16; 260];
    // SAFETY: `buf` is a live, writable buffer of exactly the length passed; the call writes at
    // most that many units and returns how many it wrote (excluding the NUL).
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    (len > 0 && len < buf.len()).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buf[..len])))
}

/// A file, or an app execution alias (the reparse points under
/// `%LOCALAPPDATA%\Microsoft\WindowsApps`, which `metadata` / `is_file` can't follow, so they'd
/// read as gone).
pub(crate) fn is_file_or_alias(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| !meta.is_dir())
}
