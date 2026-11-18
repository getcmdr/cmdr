//! The app's operation progress as the OS shell shows it: a bar on the Dock
//! tile (macOS), the taskbar button (Windows), or the launcher entry (Linux).
//!
//! `operations_host` feeds it the engine's facts (`note_*`), [`aggregate`]
//! folds the queue into one [`ShellProgress`], and a worker thread hands each
//! change to the OS through the [`platform`] seam, throttled.

mod aggregate;
mod platform;

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use cmdr_operations::{OperationSnapshot, WriteProgressEvent};
use tauri::{AppHandle, Manager};

use crate::ignore_poison::IgnorePoison;
use aggregate::{Aggregate, ShellProgress};

/// The shortest gap between two OS updates. The bar moves in whole percents,
/// so a big copy changes it at most about once a second anyway; this caps the
/// burst when many small operations start and end together.
const MIN_UPDATE_GAP: Duration = Duration::from_millis(500);

/// The window whose taskbar button carries the bar on Windows. On macOS and
/// Linux the bar is app-wide and any window reaches it.
const MAIN_WINDOW_LABEL: &str = "main";

/// The aggregate and the worker's wake-up line, in Tauri's managed state.
struct ShellProgressHub {
    state: Mutex<HubState>,
    wake: Sender<()>,
}

struct HubState {
    aggregate: Aggregate,
    /// The summary as of the last fact, so a tick that changes nothing visible
    /// doesn't wake the worker.
    current: ShellProgress,
}

/// Starts the shell progress: manages the hub and spawns its worker. Runs in
/// `setup()` before `operations_host::install`, so no fact arrives before it.
pub(crate) fn install(app: &AppHandle) {
    let (wake, woken) = mpsc::channel();
    app.manage(ShellProgressHub {
        state: Mutex::new(HubState {
            aggregate: Aggregate::default(),
            current: ShellProgress::Hidden,
        }),
        wake,
    });
    let worker_app = app.clone();
    if let Err(e) = thread::Builder::new()
        .name("shell-progress".into())
        .spawn(move || run_worker(&worker_app, &woken))
    {
        log::warn!(target: "shell_progress", "couldn't start the shell progress worker, so no Dock or taskbar bar this session: {e}");
    }
}

/// An `operations-changed` snapshot: which operations exist and their status.
pub(crate) fn note_operations(app: &AppHandle, operations: &[OperationSnapshot]) {
    update(app, |aggregate| aggregate.note_operations(operations));
}

/// A `write-progress` tick.
pub(crate) fn note_progress(app: &AppHandle, event: &WriteProgressEvent) {
    update(app, |aggregate| aggregate.note_progress(event));
}

/// A `write-complete`: the operation's work is all done.
pub(crate) fn note_completed(app: &AppHandle, operation_id: &str) {
    update(app, |aggregate| aggregate.note_completed(operation_id));
}

/// Applies one fact and wakes the worker if the summary changed. A no-op
/// before `install` (a test app, or a fact racing setup).
fn update(app: &AppHandle, apply: impl FnOnce(&mut Aggregate)) {
    let Some(hub) = app.try_state::<ShellProgressHub>() else {
        return;
    };
    let changed = {
        let mut state = hub.state.lock_ignore_poison();
        apply(&mut state.aggregate);
        let next = state.aggregate.summary();
        let changed = next != state.current;
        state.current = next;
        changed
    };
    if changed {
        // A send fails only once the worker is gone, and then there's no one to tell.
        let _ = hub.wake.send(());
    }
}

/// Waits for a change, holds off until [`MIN_UPDATE_GAP`] since the last OS
/// update, then shows the summary as it stands by then. Blocks on the channel
/// between changes: no polling.
fn run_worker(app: &AppHandle, woken: &Receiver<()>) {
    let mut shown = ShellProgress::Hidden;
    let mut last_update: Option<Instant> = None;
    while woken.recv().is_ok() {
        let wait = remaining_gap(last_update, Instant::now());
        if !wait.is_zero() {
            thread::sleep(wait);
        }
        // Every wake-up that landed meanwhile is answered by this one read.
        while woken.try_recv().is_ok() {}
        let Some(hub) = app.try_state::<ShellProgressHub>() else {
            return;
        };
        let next = hub.state.lock_ignore_poison().current;
        if next == shown {
            continue;
        }
        show(app, next);
        shown = next;
        last_update = Some(Instant::now());
    }
}

/// How long to hold off before the next OS update.
fn remaining_gap(last_update: Option<Instant>, now: Instant) -> Duration {
    last_update.map_or(Duration::ZERO, |at| {
        MIN_UPDATE_GAP.saturating_sub(now.saturating_duration_since(at))
    })
}

fn show(app: &AppHandle, progress: ShellProgress) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        log::debug!(target: "shell_progress", "no main window to carry {progress:?}");
        return;
    };
    log::debug!(target: "shell_progress", "showing {progress:?}");
    if let Err(e) = window.set_progress_bar(platform::bar_state(progress)) {
        log::warn!(target: "shell_progress", "couldn't show {progress:?} on the OS progress bar: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_update_goes_out_at_once() {
        assert_eq!(remaining_gap(None, Instant::now()), Duration::ZERO);
    }

    #[test]
    fn a_quick_follow_up_waits_out_the_rest_of_the_gap() {
        let now = Instant::now();
        let last = now
            .checked_sub(Duration::from_millis(200))
            .expect("the clock is past 200 ms");
        assert_eq!(remaining_gap(Some(last), now), Duration::from_millis(300));
    }

    #[test]
    fn a_late_follow_up_goes_out_at_once() {
        let now = Instant::now();
        let last = now.checked_sub(Duration::from_secs(2)).expect("the clock is past 2 s");
        assert_eq!(remaining_gap(Some(last), now), Duration::ZERO);
    }
}
