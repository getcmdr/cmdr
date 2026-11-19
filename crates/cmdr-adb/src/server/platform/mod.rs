//! Where this OS keeps `adb`, what makes a file runnable here, and how a helper process starts
//! quietly. A platform seam (`docs/guides/platform-seams.md`); the per-OS lists themselves are
//! data in `super::binary_search`, so every OS's list is tested on every OS.

use std::path::Path;

use super::binary_search::AdbSearch;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

/// What macOS and Linux share: the executable bit, and nothing to hide.
#[cfg(unix)]
mod unix;

const _: AdbSearch = HOST_SEARCH;
const _: fn(&Path) -> bool = is_executable;
const _: fn(&mut tokio::process::Command) = start_quietly;

/// This OS's search: its binary name, default SDK, and package-manager folders.
pub(super) use imp::HOST_SEARCH;

/// Whether `path` is a file this OS would run.
pub(super) use imp::is_executable;

/// Makes `command` start without any window of its own: `adb` is a console tool, and Cmdr is a
/// GUI app with no console for it to borrow.
pub(super) use imp::start_quietly;
