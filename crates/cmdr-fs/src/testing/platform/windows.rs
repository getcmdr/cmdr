//! Windows links need a privilege a plain user only has with Developer Mode, and their
//! kind (file or folder) is fixed at creation.

use std::io;
use std::path::Path;
use std::sync::OnceLock;

/// See the re-export in `mod.rs`.
pub fn symlink<T: AsRef<Path>, L: AsRef<Path>>(target: T, link: L) -> io::Result<()> {
    let (target, link) = (target.as_ref(), link.as_ref());
    let resolved = match link.parent() {
        Some(folder) if target.is_relative() => folder.join(target),
        _ => target.to_path_buf(),
    };
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

/// An elevated Windows process still meets ACL refusals the way a test means it to, so
/// nothing here needs skipping.
pub fn is_root() -> bool {
    false
}

/// See the re-export in `mod.rs`.
pub fn long_path(path: &Path) -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 512];
    loop {
        let capacity = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: `wide` is NUL-terminated and outlives the call; `buf` is writable for
        // `capacity` units, the size the call is told.
        let written = unsafe { GetLongPathNameW(wide.as_ptr(), buf.as_mut_ptr(), capacity) } as usize;
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        if written < buf.len() {
            buf.truncate(written);
            return Ok(std::ffi::OsString::from_wide(&buf).into());
        }
        // Too small: `written` is the size it needs, its NUL included.
        buf.resize(written, 0);
    }
}

/// Probed once per process: one dangling file link in a fresh temp folder.
pub fn can_create_symlinks() -> bool {
    static CAN: OnceLock<bool> = OnceLock::new();
    *CAN.get_or_init(|| {
        let Ok(dir) = tempfile::tempdir() else { return false };
        std::os::windows::fs::symlink_file("missing", dir.path().join("probe")).is_ok()
    })
}
