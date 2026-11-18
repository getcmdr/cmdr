//! Picks this OS's user database for the owner and group columns.

#[cfg(unix)]
mod unix;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: fn(u32) -> Option<String> = user_name_of;
const _: fn(u32) -> Option<String> = group_name_of;

/// The name of user `uid`, or `None` where this OS has no such user (the column then shows the
/// number).
pub(super) use imp::user_name_of;

/// The name of group `gid`, or `None` where this OS has no such group.
pub(super) use imp::group_name_of;
