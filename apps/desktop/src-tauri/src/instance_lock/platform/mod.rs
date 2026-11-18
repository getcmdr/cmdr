//! How each OS shows the "already running" alert before any window of ours is up. A platform
//! seam (`docs/guides/platform-seams.md`).

use std::time::Duration;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(&str, &str, Duration) = show_alert;

/// Shows `title` and `body` with a single OK and blocks this thread until it's dismissed, it times
/// out after `timeout`, or the OS has no surface (then it returns at once). ❌ Never pumps the
/// calling thread's event loop: that's Tauri's, and a refused process must not keep booting behind
/// the alert. The caller's watchdog ends the process whatever happens here.
pub(super) use imp::show_alert;
