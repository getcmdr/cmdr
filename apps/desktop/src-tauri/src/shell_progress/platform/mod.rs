//! How each OS draws the app's progress: the Dock tile on macOS, the main
//! window's taskbar button on Windows, the launcher entry on Linux. All three
//! go through Tauri's `set_progress_bar`; the arms differ in what each OS can
//! show. A platform seam: `docs/guides/platform-seams.md`.

use tauri::window::{ProgressBarState, ProgressBarStatus};

use super::aggregate::{ShellProgress, Tone};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(ShellProgress) -> ProgressBarState = bar_state;

/// The bar this OS draws for `progress`. Always carries both a status and a
/// value: tao keeps the previous value when one is missing, so a half-set
/// state would show a stale number.
pub(super) use imp::bar_state;

/// The status for a measured bar of this tone, the same on every OS (Linux
/// draws all three alike).
fn tone_status(tone: Tone) -> ProgressBarStatus {
    match tone {
        Tone::Normal => ProgressBarStatus::Normal,
        Tone::Paused => ProgressBarStatus::Paused,
        Tone::Error => ProgressBarStatus::Error,
    }
}

/// No bar at all.
fn hidden() -> ProgressBarState {
    ProgressBarState {
        status: Some(ProgressBarStatus::None),
        progress: Some(0),
    }
}

/// A bar of `tone` at `percent`.
fn shown(tone: Tone, percent: u8) -> ProgressBarState {
    ProgressBarState {
        status: Some(tone_status(tone)),
        progress: Some(u64::from(percent)),
    }
}

// Every arm is a pure mapping over portable Tauri types, so the tests load all
// three on any host: the Windows arm runs here long before a Windows lane can.
#[cfg(test)]
#[allow(
    clippy::duplicate_mod,
    reason = "the host's own arm is also `imp`; the tests run all three"
)]
#[path = "linux.rs"]
mod linux_arm;
#[cfg(test)]
#[allow(
    clippy::duplicate_mod,
    reason = "the host's own arm is also `imp`; the tests run all three"
)]
#[path = "macos.rs"]
mod macos_arm;
#[cfg(test)]
#[allow(
    clippy::duplicate_mod,
    reason = "the host's own arm is also `imp`; the tests run all three"
)]
#[path = "windows.rs"]
mod windows_arm;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
