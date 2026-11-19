//! Picks this OS's PAC runner. Only macOS and Windows run PAC files (`crate::pac` is built for
//! those two); Linux reads no system proxy layer at all.

use std::time::Duration;

use crate::entries::{Entry, PacSource};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(&PacSource, &str, Duration) -> Option<Vec<Entry>> = execute;

/// Runs the PAC `source` names for `url` on THIS thread, giving up after about `timeout`: the
/// ordered list it answers, or `None` when it couldn't run (unreachable, broken, too slow, or a
/// source this OS has no runner for).
pub(super) use imp::execute;

/// Reads and frees a WinHTTP-allocated string, for the Windows system arm's settings read too.
#[cfg(target_os = "windows")]
pub(crate) use imp::take_global_string;
