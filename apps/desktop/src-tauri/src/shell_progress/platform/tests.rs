//! Each arm's mapping, run on every host (`mod.rs` loads all three).

use tauri::window::{ProgressBarState, ProgressBarStatus};

use super::super::aggregate::{ShellProgress, Tone};
use super::{linux_arm, macos_arm, windows_arm};

type Arm = fn(ShellProgress) -> ProgressBarState;
const ARMS: [(&str, Arm); 3] = [
    ("macos", macos_arm::bar_state),
    ("linux", linux_arm::bar_state),
    ("windows", windows_arm::bar_state),
];

fn status_of(state: &ProgressBarState) -> &'static str {
    match state.status {
        Some(ProgressBarStatus::None) => "none",
        Some(ProgressBarStatus::Normal) => "normal",
        Some(ProgressBarStatus::Indeterminate) => "indeterminate",
        Some(ProgressBarStatus::Paused) => "paused",
        Some(ProgressBarStatus::Error) => "error",
        None => "unset",
    }
}

#[test]
fn every_arm_hides_the_bar_with_a_value_set() {
    for (os, arm) in ARMS {
        let state = arm(ShellProgress::Hidden);
        assert_eq!(status_of(&state), "none", "{os}");
        assert_eq!(state.progress, Some(0), "{os}");
    }
}

#[test]
fn every_arm_draws_a_measured_bar_in_its_tone() {
    for (os, arm) in ARMS {
        for (tone, status) in [
            (Tone::Normal, "normal"),
            (Tone::Paused, "paused"),
            (Tone::Error, "error"),
        ] {
            let state = arm(ShellProgress::Shown {
                percent: Some(42),
                tone,
            });
            assert_eq!(status_of(&state), status, "{os} {tone:?}");
            assert_eq!(state.progress, Some(42), "{os} {tone:?}");
        }
    }
}

#[test]
fn macos_and_linux_show_an_unmeasured_start_as_an_empty_bar() {
    // Neither has an indeterminate look: tao would draw `Indeterminate` as a
    // normal bar at whatever value it last held.
    for arm in [macos_arm::bar_state as Arm, linux_arm::bar_state] {
        let state = arm(ShellProgress::Shown {
            percent: None,
            tone: Tone::Normal,
        });
        assert_eq!(status_of(&state), "normal");
        assert_eq!(state.progress, Some(0));
    }
}

#[test]
fn windows_shows_an_unmeasured_start_as_a_marquee() {
    let state = windows_arm::bar_state(ShellProgress::Shown {
        percent: None,
        tone: Tone::Normal,
    });
    assert_eq!(status_of(&state), "indeterminate");
}

#[test]
fn windows_shows_an_unmeasured_paused_or_failed_start_as_a_full_colored_bar() {
    for (tone, status) in [(Tone::Paused, "paused"), (Tone::Error, "error")] {
        let state = windows_arm::bar_state(ShellProgress::Shown { percent: None, tone });
        assert_eq!(status_of(&state), status, "{tone:?}");
        assert_eq!(state.progress, Some(100), "{tone:?}");
    }
}
