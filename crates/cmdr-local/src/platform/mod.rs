//! The local disk's `Volume`, one sibling per OS family: `LocalPosixVolume` on macOS and
//! Linux, `LocalWindowsVolume` on Windows.
//!
//! Siblings, not one type with Windows arms: a POSIX volume is
//! `lstat`, `renamex_np`/`renameat2`, modes, and xattrs through and through, and a Windows one
//! is `\\?\` paths, `MoveFileExW`, attributes, and reparse points. Each OS's callers name the
//! one it has; what both answer the same way is pinned here.
//!
//! **Which name to write.** `LocalVolume` is this OS's sibling: name it wherever the code
//! means "this machine's local disk", whatever the OS (a pane's root, a folder opened as its
//! own volume, a test's tempdir rig). Name `LocalPosixVolume` or `LocalWindowsVolume` only
//! where the code depends on that OS family: modes, xattrs, inodes, errnos, `std::os::unix`,
//! or the POSIX-only inherent methods (`barriers_issued`, `barriers`, `with_barrier_interval`,
//! `with_sidecar_probe`).

use std::io;
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::volume::MountClass;
use cmdr_fs::volume::host::VolumeHost;

mod anchor;
#[cfg(unix)]
pub(crate) mod local_posix;
#[cfg(target_os = "windows")]
pub(crate) mod local_windows;
mod short_names;

#[cfg(unix)]
pub use local_posix::LocalPosixVolume;
#[cfg(target_os = "windows")]
pub use local_windows::LocalWindowsVolume;
/// Whether a Windows drive keeps a Recycle Bin: the one answer the volume's `has_trash`, the
/// app's switcher row, and the Recycle Bin arm's refusal read (`local_windows/recycle_bin.rs`).
#[cfg(target_os = "windows")]
pub use local_windows::recycle_bin;

/// This machine's local disk as a `Volume`: `LocalPosixVolume` on macOS and Linux. When to
/// name it and when the sibling: the module doc.
#[cfg(unix)]
pub type LocalVolume = LocalPosixVolume;
/// This machine's local disk as a `Volume`: `LocalWindowsVolume` on Windows. When to name it
/// and when the sibling: the module doc.
#[cfg(target_os = "windows")]
pub type LocalVolume = LocalWindowsVolume;

#[cfg(unix)]
use local_posix as imp;
#[cfg(target_os = "windows")]
use local_windows as imp;

const _: fn(&Path, &Path) -> io::Result<()> = rename_local_exclusive;
const _: fn(&AppPath, &AppPath) -> io::Result<()> = rename_local_exclusive_at;

// The constructors every `LocalVolume` caller uses, with one signature on every OS, so a
// site written on one OS builds on the other. The `_at` forms take the root as an app path;
// the native ones go once their callers move (plan milestones 10a–10d).
const _: fn(String, PathBuf, VolumeHost) -> LocalVolume =
    |name, root, host| LocalVolume::local_folder(name, root, host);
const _: fn(String, PathBuf, MountClass, VolumeHost) -> LocalVolume =
    |name, root, mount, host| LocalVolume::on_mount(name, root, mount, host);
const _: fn(String, AppPathBuf, VolumeHost) -> LocalVolume =
    |name, root, host| LocalVolume::local_folder_at(name, root, host);
const _: fn(String, AppPathBuf, MountClass, VolumeHost) -> LocalVolume =
    |name, root, mount, host| LocalVolume::on_mount_at(name, root, mount, host);
#[cfg(any(test, feature = "testing"))]
const _: fn(LocalVolume, crate::reading::ListingHook) -> LocalVolume = |volume, hook| volume.with_listing_hook(hook);

/// Renames a local path only when `destination` is unoccupied.
///
/// **The one no-replace rename primitive in the app**: every rename that isn't
/// meant to overwrite comes through here, from the pane's F2 to a copy landing
/// its temp on a name the conflict check found free. A plain rename replaces
/// silently on every OS (`rename(2)`, and `std::fs::rename` on Windows, which asks
/// for `MOVEFILE_REPLACE_EXISTING`), and both callers have a window between the
/// check and the rename in which a file can appear, so destroying it would be
/// silent and unrecoverable.
///
/// Takes native paths (on Windows either spelling, `C:\x` or the app path's `C:/x`); its
/// callers move to [`rename_local_exclusive_at`] with their crate (plan milestone 10a).
pub use imp::rename_local_exclusive;

/// [`rename_local_exclusive`] of two app paths, crossing to the OS at `AppPath::to_native`.
///
/// # Errors
///
/// `InvalidInput` (or `InvalidFilename` for a name Windows can't store) carrying the typed
/// refusal of a path no OS call opens, else what the rename answers.
pub use imp::rename_local_exclusive_at;
