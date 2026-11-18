//! The Windows no-replace rename: `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`.
//!
//! ❗ `std::fs::rename` REPLACES an existing file on Windows (it asks for
//! `MOVEFILE_REPLACE_EXISTING`, or `FILE_RENAME_FLAG_REPLACE_IF_EXISTS`), so it's the force
//! path only. Without the flag the filesystem refuses an occupied name in the same kernel
//! step that renames (`ERROR_ALREADY_EXISTS`, which std reads as `AlreadyExists`), so there
//! is no check-then-rename window to lose a file in. No `MOVEFILE_COPY_ALLOWED` either: a
//! rename across volumes is refused (`ERROR_NOT_SAME_DEVICE`) rather than turned into a copy
//! and delete.

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

use super::paths::native;

/// The Windows arm of `crate::rename_local_exclusive`. Takes app paths.
pub fn rename_local_exclusive(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide(&native(source)?);
    let destination = wide(&native(destination)?);
    // SAFETY: both buffers are NUL-terminated UTF-16 that outlive the call, which only reads them.
    let ok = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `path` as a NUL-terminated UTF-16 string for a `W` call.
pub(crate) fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}
