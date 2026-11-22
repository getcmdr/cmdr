//! The local streams' per-OS half: the version a resumed read compares, the positional read,
//! the owner-only create mode, and how an error names its path. The stream bodies stay in
//! `streams.rs`, the same on every OS.

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::path::Path;

use cmdr_fs::volume::VolumeError;

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

/// Which version of a local file a read saw, what a resumed read compares
/// (`Volume::resume_read_stream`).
pub(super) use imp::LocalVersion;
/// Makes a new file owner-only while it streams (`0o600` on POSIX). Windows has no mode, so
/// a new file takes its folder's inherited access there.
pub(super) use imp::owner_only;
/// One positional read at `offset` (`pread`; `ReadFile` with an offset on Windows), which
/// may come back short.
pub(super) use imp::read_at;
/// The path an error or a refusal names: the OS path's text on POSIX, the app path on
/// Windows (`C:/x`, never the `\\?\` form a body opens).
pub(super) use imp::shown;

const _: fn(&File, &Metadata, &Path) -> LocalVersion = LocalVersion::of;
const _: fn(&File, &mut [u8], u64) -> io::Result<usize> = read_at;
const _: fn(&mut OpenOptions) = owner_only;
const _: fn(&Path) -> String = shown;

/// `err` as a `VolumeError` naming [`shown`]`(os)`.
pub(super) fn error_at(err: &io::Error, os: &Path) -> VolumeError {
    VolumeError::from_io_at(err, shown(os))
}
