//! Linux: tao sets the Unity `LauncherEntry` progress through `libunity`,
//! which GNOME's Ubuntu Dock, KDE's task manager, and Plank read; elsewhere
//! (or without `libunity`) it's a no-op. The API has a value and a visibility,
//! no colors and no indeterminate look, so paused and error draw like normal
//! and an unmeasured start shows an empty bar (tao 0.37.1,
//! `platform_impl/linux/taskbar.rs`, read 2026-11-18).

use tauri::window::ProgressBarState;

use super::super::aggregate::ShellProgress;

pub(crate) fn bar_state(progress: ShellProgress) -> ProgressBarState {
    match progress {
        ShellProgress::Hidden => super::hidden(),
        ShellProgress::Shown { percent, tone } => super::shown(tone, percent.unwrap_or(0)),
    }
}
