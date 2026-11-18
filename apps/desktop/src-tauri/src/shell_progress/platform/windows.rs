//! Windows: the main window's taskbar button, through `ITaskbarList3`
//! (`SetProgressState` + `SetProgressValue`, tao 0.37.1). It has every look:
//! green, yellow when paused, red on error, and a moving marquee for an
//! unmeasured start. A marquee has no color, so an unmeasured start that is
//! paused shows a full yellow bar instead: an empty one would show nothing
//! at all.

use tauri::window::{ProgressBarState, ProgressBarStatus};

use super::super::aggregate::{ShellProgress, Tone};

pub(crate) fn bar_state(progress: ShellProgress) -> ProgressBarState {
    match progress {
        ShellProgress::Hidden => super::hidden(),
        ShellProgress::Shown {
            percent: Some(percent),
            tone,
        } => super::shown(tone, percent),
        ShellProgress::Shown {
            percent: None,
            tone: Tone::Normal,
        } => ProgressBarState {
            status: Some(ProgressBarStatus::Indeterminate),
            progress: Some(0),
        },
        ShellProgress::Shown { percent: None, tone } => super::shown(tone, 100),
    }
}
