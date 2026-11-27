//! Picks this OS's way of raising the soft open-file limit.

use super::OpenFileLimit;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;
#[cfg(unix)]
mod unix;

const _: fn() -> OpenFileLimit = raise;

/// Raises the soft limit once; never fails loudly (the answer says what happened).
pub(super) use imp::raise;
