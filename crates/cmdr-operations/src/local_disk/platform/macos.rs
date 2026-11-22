//! macOS: the POSIX answers, plus BSD flags, xattrs, and NSURL's purgeable-space figure.

use std::path::Path;

pub(crate) use super::unix::{
    create_dir_with_mode, folder_volume, free_bytes, mode_of, remove_leaf, set_mode, symlink, with_mode, write_probe,
};

/// What `lstat` says of a path's BSD flags (`st_flags`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BsdFlags {
    /// The flags `lstat` read.
    Read(u32),
    /// `lstat` failed (the entry may be gone).
    Unreadable,
    /// The path holds an interior NUL, so the OS can't be handed it.
    Unrepresentable,
}

/// `lstat(path).st_flags`.
pub(crate) fn bsd_flags(path: &Path) -> BsdFlags {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return BsdFlags::Unrepresentable;
    };
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `c_path` is a valid NUL-terminated C string and `stat` a valid pointer to
    // writable memory the size of a `libc::stat`.
    let result = unsafe { libc::lstat(c_path.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return BsdFlags::Unreadable;
    }
    // SAFETY: `lstat` returned 0, so it filled the whole struct.
    BsdFlags::Read(unsafe { stat.assume_init() }.st_flags)
}

/// The item's own lock flags: Finder's Locked (`chflags uchg`), and the system and
/// append-only kinds.
pub(crate) fn is_locked_item(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;

    const LOCKS: u32 = libc::UF_IMMUTABLE | libc::UF_APPEND | libc::SF_IMMUTABLE | libc::SF_APPEND;
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.st_flags() & LOCKS != 0)
}

/// macOS refuses a Locked item's unlink and rename itself (`EPERM`), and that refusal
/// keeps its own flow, so nothing is refused up front here.
pub(crate) fn locked_against_removal(_path: &Path) -> bool {
    false
}

pub(crate) const ASKS_ABOUT_LOCKS: bool = false;

/// `NSURLVolumeAvailableCapacityForImportantUsageKey`: free space including
/// what macOS purges on demand, matching Finder and the status bar.
pub(crate) fn important_free_bytes(path: &Path) -> Option<u64> {
    cmdr_local::space::get_volume_space(&path.to_string_lossy())?.available_bytes()
}

/// The extended attribute `name` on `path` (`xattr::get`).
pub(crate) fn read_xattr(path: &Path, name: &str) -> std::io::Result<Option<Vec<u8>>> {
    xattr::get(path, name)
}
