//! What an app path asks of the host: its flavor and namer, how its `OsStr` spells a name,
//! and (macOS, Linux, Windows) the `PathBuf` crossing itself. The conversions are pure and
//! live above the seam; each arm only moves bytes or UTF-16 units in and out of `OsStr`.

use std::borrow::Cow;
use std::ffi::OsStr;

use super::{Namer, PathFlavor};

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: PathFlavor = HOST_FLAVOR;
const _: Namer = HOST_NAMER;
const _: fn(&OsStr) -> Cow<'_, str> = os_name_to_app;
const _: fn(&str) -> Cow<'_, OsStr> = app_to_os;
#[cfg(cmdr_host)]
const _: fn(&super::AppPath) -> Result<std::path::PathBuf, super::NativeRefusal> = super::AppPath::to_native;
#[cfg(cmdr_host)]
const _: fn(&super::AppPath) -> Result<std::path::PathBuf, super::NativeRefusal> = super::AppPath::to_native_plain;
#[cfg(cmdr_host)]
const _: fn(&std::path::Path) -> Result<super::AppPathBuf, super::NotAnAppPath> = super::AppPathBuf::from_native;
#[cfg(cmdr_host)]
const _: fn(&std::path::Path, PathFlavor) -> Result<super::AppPathBuf, super::NotAnAppPath> =
    super::AppPathBuf::from_native_as;

/// This OS's flavor: drives and UNC shares on Windows, `/` elsewhere (the browser build's
/// demo tree is POSIX).
pub(super) use imp::HOST_FLAVOR;

/// How this host's own disks name things: UTF-16 units on Windows, bytes elsewhere.
pub(super) use imp::HOST_NAMER;

/// The app spelling of `os` (a name or a whole path): its text, every unit the host's
/// decoder rejects escaped; borrowed when it's valid Unicode text holding no escape-range
/// code point.
pub(super) use imp::os_name_to_app;

/// The `OsStr` an app spelling stands for, every escape of this host's alphabet decoded back
/// to its byte or unit; borrowed when there's none.
pub(super) use imp::app_to_os;
