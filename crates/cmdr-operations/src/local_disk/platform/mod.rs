//! The local-disk questions whose answer differs per OS, one arm per OS plus the
//! browser build's (`docs/guides/platform-seams.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;

/// What the OS said when asked whether this process may write a path
/// ([`write_probe`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteProbe {
    /// This process may write there.
    Allowed,
    /// Refused, with the OS's code (`EACCES` when a POSIX OS gave none; a Win32
    /// code on Windows).
    Refused(i32),
    /// The path holds an interior NUL, so the OS can't be handed it.
    Unrepresentable,
    /// The OS couldn't say (the browser build has no disk; on Windows, another
    /// program holds the item without sharing writes), so the write itself
    /// decides. Every caller reads it as a pre-flight that passed, which is safe
    /// because a pre-flight only words a refusal early.
    #[cfg_attr(unix, expect(dead_code, reason = "`access(W_OK)` always answers on macOS and Linux"))]
    Unknown,
}

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;
#[cfg(unix)]
mod unix;

const _: fn(&std::fs::Permissions) -> u32 = mode_of;
const _: fn(&Path, u32) -> std::io::Result<()> = set_mode;
const _: fn(&Path, u32) -> std::io::Result<()> = create_dir_with_mode;
const _: fn(&mut std::fs::OpenOptions, u32) -> &mut std::fs::OpenOptions = with_mode;
const _: fn(&Path, &Path) -> std::io::Result<()> = symlink;
const _: fn(&Path) -> WriteProbe = write_probe;
const _: fn(&Path) -> Option<u64> = free_bytes;
const _: fn(&Path) -> Option<u64> = important_free_bytes;
const _: fn(&Path) -> bool = is_locked_item;
const _: fn(&Path) -> bool = locked_against_removal;
const _: bool = ASKS_ABOUT_LOCKS;
const _: fn(&str, PathBuf, VolumeHost) -> Arc<dyn Volume> = folder_volume;

/// Whether [`locked_against_removal`] can ever answer `true` here, so a caller off the
/// worker thread (the volume delete) skips the per-file hop where it never would.
pub(crate) use imp::ASKS_ABOUT_LOCKS;
/// `mkdir(dir, mode)`; off POSIX the mode is dropped.
pub(crate) use imp::create_dir_with_mode;
/// A volume over the local folder `root`, for a working copy or a spool.
pub(crate) use imp::folder_volume;
/// The destination's physically free bytes for this user, the instant figure.
pub(crate) use imp::free_bytes;
/// Free bytes counting what the OS frees on demand (macOS's purgeable space): the
/// slow, honest figure, asked only when [`free_bytes`] falls short. `None` where
/// there's no such figure.
pub(crate) use imp::important_free_bytes;
/// Whether `path` itself carries a lock flag: a fact about the item, never a guess
/// from a path or a message.
pub(crate) use imp::is_locked_item;
/// Whether `path` wears a lock the OS would otherwise walk past, so a permanent delete
/// or an Overwrite refuses it up front as `FileLocked` (the Locked advice): Windows'
/// read-only attribute on a FILE, which `std::fs::remove_file` deletes through and a
/// rename carries aside. `false` on macOS and Linux, whose locks the OS refuses itself
/// (their refusal keeps its own wording). One `lstat`, asked per file, Windows only.
pub(crate) use imp::locked_against_removal;
/// The permission bits of `permissions` (`st_mode`); off POSIX, `0o444` or `0o644`
/// by the read-only flag.
pub(crate) use imp::mode_of;
/// `chmod(path, mode)`.
pub(crate) use imp::set_mode;
/// `symlink(target, link)`.
pub(crate) use imp::symlink;
/// `options` opening a new file with `mode`; off POSIX the mode is dropped.
pub(crate) use imp::with_mode;
/// Whether this process may write `path`, asked of the OS. ❗ A syscall: on a hung
/// network mount it blocks like any.
pub(crate) use imp::write_probe;

#[cfg(target_os = "macos")]
pub(crate) use imp::{BsdFlags, bsd_flags, read_xattr};
