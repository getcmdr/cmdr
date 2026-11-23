//! Picks this OS's way to make a file private to the current user at creation: a create mode on
//! Unix, a security descriptor on Windows.

use std::fs::File;
use std::io;
use std::path::Path;

#[cfg(unix)]
mod unix;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: fn(&Path) -> io::Result<File> = create_new;
const _: fn(&Path) -> io::Result<bool> = is_private_to_current_user;

/// A new file at `path`, opened for writing, that only the current user can open from the moment
/// it exists. Fails on anything already at `path`, without following a link there.
pub(super) use imp::create_new;

/// Whether the regular file at `path` is open to the current user alone.
pub(super) use imp::is_private_to_current_user;

/// What a Windows file's security descriptor says, for the test that asserts every entry.
#[cfg(all(test, target_os = "windows"))]
pub(super) use imp::{AccessEntry, current_user_sid, file_security};
