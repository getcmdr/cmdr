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
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread::JoinHandle;

use log::{error, info, warn};
use tauri::AppHandle;

use crate::ignore_poison::IgnorePoison;

/// What the window hands the worker.
enum Job {
    /// Re-read the drive table and announce what changed.
    Rescan,
    /// A removal or lock query for the handle registered as `registration`: let go of the
    /// drive, then answer on `done` (the window answers Windows only after).
    LetGo { registration: usize, done: SyncSender<()> },
    /// The removal was refused, or the lock ended: take the drive back.
    TakeBack { registration: usize },
    /// The drive is going or gone: drop its handle and registration.
    Gone { registration: usize },
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
    window_thread: Option<JoinHandle<()>>,
    worker_thread: Option<JoinHandle<()>>,
    /// Answers once the worker has read its baseline: a test waits on it before changing a
    /// drive, the app never does.
    #[cfg_attr(not(test), expect(dead_code, reason = "only a test waits for the baseline"))]
    baseline: mpsc::Receiver<()>,
}

impl DriveWatch {
    /// Starts both threads and returns once the window is up. The worker reads the known drives
    /// first, off the caller's thread (the setup hook's), so only what changes after that is
    /// news.
    fn start(announce: impl Announce) -> Result<Self, String> {
        let (jobs, inbox) = mpsc::channel();
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let window_jobs = jobs.clone();
        let window_thread = std::thread::Builder::new()
            .name("drive-events".to_owned())
            .spawn(move || window::run(window_jobs, ready_tx))
            .map_err(|e| format!("couldn't start the drive events thread: {e}"))?;
        let window = match ready.recv() {
            Ok(Ok(window)) => window,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("the drive events thread ended before its window was up".to_owned()),
        };
        let worker = worker::Worker::new(window, announce);
        let (baseline_tx, baseline) = mpsc::sync_channel(1);
        let worker_thread = std::thread::Builder::new()
            .name("drive-events-worker".to_owned())
            .spawn(move || worker.run(&inbox, &baseline_tx))
            .map_err(|e| {
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
        if let Some(thread) = self.window_thread.take()
            && thread.join().is_err()
        {
            error!(target: "volumes::windows", "The drive events thread panicked");
        }
        // a worker that already ended needs no stop
        let _ = self.jobs.send(Job::Stop);
        if let Some(thread) = self.worker_thread.take()
            && thread.join().is_err()
        {
            error!(target: "volumes::windows", "The drive events worker panicked");
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

/// Stops what [`start_volume_watcher`] started. Idempotent.
pub(crate) fn stop_volume_watcher() {
    // Taken out of the lock before dropping: the drop joins both threads.
    let watch = WATCH.lock_ignore_poison().take();
    drop(watch);
}
