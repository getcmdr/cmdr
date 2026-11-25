//! Windows' drive watcher (task 39): a drive that arrives or leaves (a USB stick, an external
//! disk, a card reader's card, a mounted VHD or ISO, a `subst` drive, a mapped share) reaches
//! the volume list, the panes, and the index by subscription, never by polling, and Windows'
//! "Safely remove" works while Cmdr shows the drive.
//!
//! Two threads:
//!
//! - **`drive-events`** (`window.rs`) owns a hidden top-level window, the only kind that hears
//!   the device broadcasts (a message-only window hears none), and pumps its messages. It
//!   reads nothing from any drive: every signal becomes a [`Job`] for the worker. Arrival and
//!   removal signals settle for [`window::SETTLE_MS`] into one re-read.
//! - **`drive-events-worker`** (`worker.rs`) does the reading: it re-reads the drive table,
//!   diffs it (`cmdr_fs::volume::windows_device_change::diff_drives`), hands each change to
//!   the shared lifecycle (`../../lifecycle.rs`, the handlers macOS feeds), and keeps one
//!   handle per local drive to hear its removal query, on which it lets go of everything Cmdr
//!   holds there.
//!
//! Which source covers which case, and the evidence: `volumes/DETAILS.md` § "Windows: drive
//! arrival and removal".

mod window;
mod worker;

#[cfg(test)]
mod tests;

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

use log::{error, info, warn};
use tauri::AppHandle;

use crate::ignore_poison::IgnorePoison;

/// What the window hands the worker.
enum Job {
    /// Re-read the drive table and announce what changed.
    Rescan,
    /// A removal or lock query for the handle registered as `registration`: let go of the
    /// drive, then answer on `done` (the window answers Windows only after).
    LetGo {
        registration: usize,
        cause: cmdr_fs::volume::windows_device_change::LetGoCause,
        done: SyncSender<()>,
    },
    /// The removal was refused, or the lock ended: take the drive back.
    TakeBack { registration: usize },
    /// The drive is going or gone: drop its handle and registration.
    Gone { registration: usize },
    /// Cmdr's own eject of the drive at `root` is about to ask Windows: let go of it, then
    /// answer on `done`.
    EjectLetGo {
        volume_id: String,
        root: String,
        done: SyncSender<()>,
    },
    /// Cmdr's own eject didn't go through: take the drive back, then answer on `done`.
    EjectTakeBack {
        volume_id: String,
        root: String,
        done: SyncSender<()>,
    },
    /// A test's own drive root to hold a handle on, answering its registration.
    #[cfg(test)]
    Track {
        volume_id: String,
        root: String,
        done: SyncSender<Option<usize>>,
    },
    /// The watch is ending.
    Stop,
}

/// Where a re-read's changes go: the lifecycle in the app, a recorder in a test. Each root is
/// an app root (`E:/`, `//server/share/`).
trait Announce: Send + 'static {
    fn arrived(&self, root: &str);
    fn left(&self, root: &str);
}

/// The app's announcer: the handlers macOS feeds too, so a drive arriving or leaving does here
/// what it does there (registration, the index stop, `volume-mounted` / `volume-unmounted`, a
/// refreshed volume list).
struct Lifecycle;

impl Announce for Lifecycle {
    fn arrived(&self, root: &str) {
        super::super::lifecycle::handle_volume_mounted(root);
    }

    fn left(&self, root: &str) {
        super::super::lifecycle::handle_volume_unmounted(root);
    }
}

/// A running watch. Dropping it closes the window and waits for both threads.
struct DriveWatch {
    jobs: Sender<Job>,
    /// The window, as an integer so the watch is `Send`.
    window: isize,
    window_thread: Option<Finishing>,
    worker_thread: Option<Finishing>,
    /// Answers once the worker has read its baseline: a test waits on it before changing a
    /// drive, the app never does.
    #[cfg_attr(not(test), expect(dead_code, reason = "only a test waits for the baseline"))]
    baseline: Receiver<()>,
}

impl DriveWatch {
    /// Starts both threads and returns once the window is up. The worker reads the known drives
    /// first, off the caller's thread (the setup hook's), so only what changes after that is
    /// news.
    fn start(announce: impl Announce) -> Result<Self, String> {
        let (jobs, inbox) = mpsc::channel();
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let window_jobs = jobs.clone();
        let window_thread = Finishing::spawn("drive-events", move || window::run(window_jobs, ready_tx))
            .map_err(|e| format!("couldn't start the drive events thread: {e}"))?;
        let window = match ready.recv() {
            Ok(Ok(window)) => window,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("the drive events thread ended before its window was up".to_owned()),
        };
        let worker = worker::Worker::new(window, announce);
        let (baseline_tx, baseline) = mpsc::sync_channel(1);
        let worker_thread =
            Finishing::spawn("drive-events-worker", move || worker.run(&inbox, &baseline_tx)).map_err(|e| {
                window::close(window);
                format!("couldn't start the drive events worker: {e}")
            })?;
        Ok(Self {
            jobs,
            window,
            window_thread: Some(window_thread),
            worker_thread: Some(worker_thread),
            baseline,
        })
    }
}

impl Drop for DriveWatch {
    fn drop(&mut self) {
        window::close(self.window);
        if let Some(thread) = self.window_thread.take() {
            thread.join_within(QUIT_WAIT);
        }
        // A worker that already ended needs no stop.
        let _ = self.jobs.send(Job::Stop);
        if let Some(thread) = self.worker_thread.take() {
            thread.join_within(QUIT_WAIT);
        }
    }
}

/// How long the watch's end waits for each thread. ❗ Quit must never hang on it: the worker can
/// be mid-read of a dead mapped drive or in an index stop, so past this it's left to finish on
/// its own, and the process ending takes it.
const QUIT_WAIT: Duration = Duration::from_secs(2);

/// A thread that says when it's done, so its join can be bounded.
struct Finishing {
    name: &'static str,
    handle: JoinHandle<()>,
    done: Receiver<()>,
}

impl Finishing {
    fn spawn(name: &'static str, body: impl FnOnce() + Send + 'static) -> std::io::Result<Self> {
        let (done_tx, done) = mpsc::sync_channel(1);
        let handle = std::thread::Builder::new().name(name.to_owned()).spawn(move || {
            // Dropped on any exit, a panic included, so the joiner always hears.
            let _done = done_tx;
            body();
        })?;
        Ok(Self { name, handle, done })
    }

    fn join_within(self, wait: Duration) {
        match self.done.recv_timeout(wait) {
            // The sender drops when the body ends, so either answer means it has.
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                if self.handle.join().is_err() {
                    // allowed-log-private: a thread's own name, no user value
                    error!(target: "volumes::windows", "The {} thread panicked", self.name);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let seconds = wait.as_secs();
                // allowed-log-private: a thread's own name, no user value
                warn!(target: "volumes::windows", "The {} thread didn't end within {seconds} s; leaving it to finish on its own", self.name);
            }
        }
    }
}

/// The app's one watch, kept until [`stop_volume_watcher`].
static WATCH: Mutex<Option<DriveWatch>> = Mutex::new(None);

/// Starts watching for drives arriving and leaving, registering and announcing each. Call once,
/// from the setup hook.
pub(crate) fn start_volume_watcher(app: &AppHandle) {
    if !super::super::lifecycle::set_app_handle(app) {
        return;
    }
    match DriveWatch::start(Lifecycle) {
        Ok(watch) => {
            *WATCH.lock_ignore_poison() = Some(watch);
            info!(target: "volumes::windows", "Watching for drives arriving and leaving");
        }
        Err(e) => warn!(
            target: "volumes::windows",
            "Not watching for drives arriving and leaving ({e}); a drive plugged in shows on the next volume list refresh"
        ),
    }
}

/// How long Cmdr's eject waits for the worker's let-go or take-back. The worker answers within
/// milliseconds unless it's mid-read of a dead mapped drive or in a removal query's index stop;
/// past this the eject goes on without it.
const EJECT_WAIT: Duration = Duration::from_secs(8);

/// The eject's let-go and take-back on one watch, for a test that runs its own.
#[cfg(test)]
impl DriveWatch {
    fn let_go_for_eject(&self, volume_id: &str, root: &str) -> bool {
        ask_on(&self.jobs, |done| Job::EjectLetGo {
            volume_id: volume_id.to_owned(),
            root: root.to_owned(),
            done,
        })
    }

    fn take_back_after_eject(&self, volume_id: &str, root: &str) -> bool {
        ask_on(&self.jobs, |done| Job::EjectTakeBack {
            volume_id: volume_id.to_owned(),
            root: root.to_owned(),
            done,
        })
    }
}

/// Hands the worker `job` (built around its `done` sender) and waits up to [`EJECT_WAIT`].
fn ask_on(jobs: &Sender<Job>, job: impl FnOnce(SyncSender<()>) -> Job) -> bool {
    let (done, answered) = mpsc::sync_channel(1);
    if jobs.send(job(done)).is_err() {
        return false;
    }
    answered.recv_timeout(EJECT_WAIT).is_ok()
}

/// The app's watch's job queue, cloned out of the lock so no wait holds it (quit takes it).
fn app_jobs() -> Option<Sender<Job>> {
    WATCH.lock_ignore_poison().as_ref().map(|watch| watch.jobs.clone())
}

/// Lets go of the drive at `root` (an app root) for Cmdr's own eject: the worker's handle on it,
/// every listing watch there, and an unmount pending at the gate (`worker.rs`). Blocks until done,
/// up to [`EJECT_WAIT`]. With no watch running, the listing watches let go alone.
pub(crate) fn let_go_for_eject(volume_id: &str, root: &str) {
    let Some(jobs) = app_jobs() else {
        if let Some(native) = native_root(root) {
            let closed = crate::file_system::watcher::release::hold_drive(&native);
            info!(target: "volumes::windows", "No drive watcher; let go of {closed} listing watch(es) for the eject");
        }
        return;
    };
    let answered = ask_on(&jobs, |done| Job::EjectLetGo {
        volume_id: volume_id.to_owned(),
        root: root.to_owned(),
        done,
    });
    if !answered {
        warn!(target: "volumes::windows", "The drive watcher didn't let go in time; the eject goes ahead");
    }
}

/// Takes back what [`let_go_for_eject`] let go of: Cmdr's eject didn't go through.
pub(crate) fn take_back_after_eject(volume_id: &str, root: &str) {
    let Some(jobs) = app_jobs() else {
        if let Some(native) = native_root(root) {
            crate::file_system::watcher::release::take_back_drive(&native);
        }
        return;
    };
    let answered = ask_on(&jobs, |done| Job::EjectTakeBack {
        volume_id: volume_id.to_owned(),
        root: root.to_owned(),
        done,
    });
    if !answered {
        warn!(target: "volumes::windows", "The drive watcher didn't take the drive back in time");
    }
}

fn native_root(root: &str) -> Option<std::path::PathBuf> {
    cmdr_fs::app_path::AppPath::from_normalized(root)
        .ok()?
        .to_native_plain()
        .ok()
}

/// Stops what [`start_volume_watcher`] started. Idempotent.
pub(crate) fn stop_volume_watcher() {
    // Taken out of the lock before dropping: the drop joins both threads.
    let watch = WATCH.lock_ignore_poison().take();
    drop(watch);
}
