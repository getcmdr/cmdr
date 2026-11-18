//! The local disk's `Volume`, one sibling per OS family: `LocalPosixVolume` on macOS and
//! Linux, `LocalWindowsVolume` on Windows.
//!
//! Siblings, not one type with Windows arms (windows-port plan, task 35): a POSIX volume is
//! `lstat`, `renamex_np`/`renameat2`, modes, and xattrs through and through, and a Windows one
//! is `\\?\` paths, `MoveFileExW`, attributes, and reparse points. Each OS's callers name the
//! one it has; what both answer the same way is pinned here.

use std::io;
use std::path::Path;

#[cfg(unix)]
pub(crate) mod local_posix;
#[cfg(target_os = "windows")]
pub(crate) mod local_windows;

#[cfg(unix)]
pub use local_posix::LocalPosixVolume;
#[cfg(target_os = "windows")]
pub use local_windows::LocalWindowsVolume;

#[cfg(unix)]
use local_posix as imp;
#[cfg(target_os = "windows")]
use local_windows as imp;

const _: fn(&Path, &Path) -> io::Result<()> = rename_local_exclusive;

/// Renames a local path only when `destination` is unoccupied.
///
/// **The one no-replace rename primitive in the app**: every rename that isn't
/// meant to overwrite comes through here, from the pane's F2 to a copy landing
/// its temp on a name the conflict check found free. A plain rename replaces
/// silently on every OS (`rename(2)`, and `std::fs::rename` on Windows, which asks
/// for `MOVEFILE_REPLACE_EXISTING`), and both callers have a window between the
/// check and the rename in which a file can appear, so destroying it would be
/// silent and unrecoverable.
pub use imp::rename_local_exclusive;
