//! macOS: tao draws a bar over the Dock tile, blue, yellow when paused, red on
//! error. It has no indeterminate look (it draws `Indeterminate` as `Normal`
//! at whatever value it last held, verified in tao 0.37.1's
//! `platform_impl/macos/progress_bar.rs`, 2026-11-18), so an unmeasured start
//! shows an empty bar: started, nothing done yet.

use tauri::window::ProgressBarState;

use super::super::aggregate::ShellProgress;

pub(crate) fn bar_state(progress: ShellProgress) -> ProgressBarState {
    match progress {
        ShellProgress::Hidden => super::hidden(),
        ShellProgress::Shown { percent, tone } => super::shown(tone, percent.unwrap_or(0)),
    }
}
