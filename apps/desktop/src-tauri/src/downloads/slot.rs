//! The process-wide Downloads watcher slot and its arming state machine.
//!
//! Arming a watcher is slow work: it resolves the Downloads root and asks
//! `fseventsd` for a stream, seconds when the daemon is busy. Both
//! `refresh_runtime` callers run on the main thread (launch and window focus),
//! so a synchronous arm hung launch (#1538). The slot splits a refresh in two:
//! [`WatcherSlot::align`] decides under the lock, instantly, and hands back the
//! slow half as a [`Job`] for another thread to run.
//!
//! `Arming` is a state of its own, so a focus change while a watcher arms
//! starts nothing more, and a gate that closes meanwhile wins: the arm finds
//! its ticket superseded and stops the watcher it built.

use std::sync::Mutex;

use super::{DownloadsWatcher, WatcherError};

/// Holds the watcher, or the fact that one is arming.
pub struct WatcherSlot {
    inner: Mutex<Inner>,
}

struct Inner {
    state: SlotState,
    /// Tells an in-flight arm whether it's still the one the slot waits for.
    next_generation: u64,
}

enum SlotState {
    Stopped,
    Arming { generation: u64 },
    Running(Box<DownloadsWatcher>),
}

/// The slow half of a refresh, to run off the main thread.
#[must_use = "a job does nothing until it runs"]
pub enum Job {
    /// Build a watcher and hand it to the ticket.
    Arm(ArmTicket),
    /// Release a watcher the gate no longer allows.
    Stop(Box<DownloadsWatcher>),
}

/// The right to install the watcher one arm builds. Dropping it unfinished (a
/// panic in the arm, a thread that never spawned) puts the slot back to
/// `Stopped`, so the next refresh retries instead of waiting forever.
#[must_use = "dropping a ticket abandons the arm"]
pub struct ArmTicket {
    slot: &'static WatcherSlot,
    generation: u64,
}

impl WatcherSlot {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                state: SlotState::Stopped,
                next_generation: 0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Move toward `should_run` and return the slow work that takes, if any.
    /// Never blocks on I/O, so it's safe on the main thread.
    pub fn align(&'static self, should_run: bool) -> Option<Job> {
        let mut inner = self.lock();
        match (should_run, &inner.state) {
            (true, SlotState::Stopped) => {
                let generation = inner.next_generation;
                inner.next_generation += 1;
                inner.state = SlotState::Arming { generation };
                Some(Job::Arm(ArmTicket { slot: self, generation }))
            }
            (false, SlotState::Running(_)) => match std::mem::replace(&mut inner.state, SlotState::Stopped) {
                SlotState::Running(watcher) => Some(Job::Stop(watcher)),
                _ => unreachable!("matched Running above"),
            },
            // The in-flight arm finds its ticket superseded and stops its own watcher.
            (false, SlotState::Arming { .. }) => {
                inner.state = SlotState::Stopped;
                None
            }
            (true, SlotState::Arming { .. } | SlotState::Running(_)) | (false, SlotState::Stopped) => None,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.lock().state, SlotState::Running(_))
    }

    /// Run `f` against the running watcher, under the slot's lock: keep it
    /// cheap, since `refresh_runtime` on the main thread waits for the lock.
    pub fn with_watcher<R>(&self, f: impl FnOnce(&DownloadsWatcher) -> R) -> Option<R> {
        match &self.lock().state {
            SlotState::Running(watcher) => Some(f(watcher.as_ref())),
            SlotState::Stopped | SlotState::Arming { .. } => None,
        }
    }

    /// Test-only: put `watcher` in a stopped slot as if it had armed.
    #[cfg(test)]
    pub fn install(&self, watcher: DownloadsWatcher) {
        let mut inner = self.lock();
        assert!(
            matches!(inner.state, SlotState::Stopped),
            "install: the slot already holds a watcher; tests must run serial or scope their installs"
        );
        inner.state = SlotState::Running(Box::new(watcher));
    }

    /// Test-only: empty the slot, handing back a running watcher.
    #[cfg(test)]
    pub fn take(&self) -> Option<DownloadsWatcher> {
        match std::mem::replace(&mut self.lock().state, SlotState::Stopped) {
            SlotState::Running(watcher) => Some(*watcher),
            SlotState::Stopped | SlotState::Arming { .. } => None,
        }
    }

    #[cfg(test)]
    fn is_arming(&self) -> bool {
        matches!(self.lock().state, SlotState::Arming { .. })
    }
}

impl ArmTicket {
    /// Install what the arm built, if the slot still waits for this arm. A
    /// superseded arm (the gate closed, maybe reopened, meanwhile) stops its
    /// watcher instead. Returns the arm's error, having reset the slot so the
    /// next refresh retries.
    pub fn finish(self, armed: Result<DownloadsWatcher, WatcherError>) -> Result<(), WatcherError> {
        let mut inner = self.slot.lock();
        let current = matches!(inner.state, SlotState::Arming { generation } if generation == self.generation);
        match armed {
            Ok(watcher) if current => {
                inner.state = SlotState::Running(Box::new(watcher));
                log::info!(target: "downloads::watcher", "Downloads watcher started (FDA gate open)");
                Ok(())
            }
            Ok(watcher) => {
                drop(inner);
                log::info!(target: "downloads::watcher", "FDA gate closed while the watcher armed; stopping it");
                watcher.stop();
                Ok(())
            }
            Err(err) => {
                if current {
                    inner.state = SlotState::Stopped;
                }
                Err(err)
            }
        }
    }
}

impl Drop for ArmTicket {
    fn drop(&mut self) {
        let mut inner = self.slot.lock();
        if matches!(inner.state, SlotState::Arming { generation } if generation == self.generation) {
            inner.state = SlotState::Stopped;
        }
    }
}

impl Job {
    /// Do the slow half: build a watcher with `arm`, or stop the one released.
    pub fn run(self, arm: impl FnOnce() -> Result<DownloadsWatcher, WatcherError>) -> Result<(), WatcherError> {
        match self {
            Self::Arm(ticket) => ticket.finish(arm()),
            Self::Stop(watcher) => {
                watcher.stop();
                log::info!(target: "downloads::watcher", "Downloads watcher stopped (FDA gate closed)");
                Ok(())
            }
        }
    }
}

/// Run `job` on its own thread and return at once. What the main-thread
/// callers use; a failed arm logs and the next refresh retries.
pub fn run_in_background(job: Job, arm: impl FnOnce() -> Result<DownloadsWatcher, WatcherError> + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("downloads-watcher".into())
        .spawn(move || {
            if let Err(err) = job.run(arm) {
                log::warn!(target: "downloads::watcher", "Couldn't start the Downloads watcher: {err}");
            }
        });
    // The closure, ticket included, dropped with the error, so the slot is
    // `Stopped` again and the next refresh retries.
    if let Err(err) = spawned {
        log::warn!(target: "downloads::watcher", "Couldn't spawn the Downloads watcher thread: {err}");
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::downloads::watcher::{DownloadDetectedEvent, EventSink};
    use crate::test_support::wait_until;

    struct NullSink;

    impl EventSink for NullSink {
        fn emit(&self, _event: DownloadDetectedEvent) {}
    }

    /// Each test gets its own slot; `'static` because a ticket points back at it.
    fn fresh_slot() -> &'static WatcherSlot {
        Box::leak(Box::new(WatcherSlot::new()))
    }

    /// A watcher that arms nothing with the OS, plus its sink: the watcher's
    /// handler holds a clone, so the sink's count drops back to one exactly
    /// when the watcher is gone.
    fn watcher() -> (DownloadsWatcher, Arc<NullSink>) {
        let sink = Arc::new(NullSink);
        let as_dyn: Arc<dyn EventSink> = sink.clone();
        (
            DownloadsWatcher::scripted_at(PathBuf::from("/nonexistent-downloads"), as_dyn),
            sink,
        )
    }

    fn arm_error() -> WatcherError {
        WatcherError::Watch(notify::Error::generic("no Downloads here"))
    }

    /// The #1538 contract: a refresh hands the arm to another thread and
    /// returns while it's still running, however long it takes.
    #[test]
    fn refresh_returns_while_the_arm_is_still_running() {
        let slot = fresh_slot();
        let (release, gate) = mpsc::channel::<()>();
        let (built, _sink) = watcher();

        let job = slot.align(true).expect("a stopped slot arms");
        run_in_background(job, move || {
            gate.recv().expect("the test releases the arm");
            Ok(built)
        });

        assert!(slot.is_arming(), "the refresh returned with the arm still blocked");
        release.send(()).unwrap();
        wait_until(Duration::from_secs(5), "the armed watcher is installed", || {
            slot.is_running()
        });
    }

    #[test]
    fn a_refresh_while_arming_starts_nothing() {
        let slot = fresh_slot();
        let _arming = slot.align(true).expect("a stopped slot arms");

        assert!(
            slot.align(true).is_none(),
            "a second arm started on a rapid focus change"
        );
    }

    #[test]
    fn closing_the_gate_while_arming_stops_the_late_watcher() {
        let slot = fresh_slot();
        let job = slot.align(true).expect("a stopped slot arms");

        assert!(slot.align(false).is_none(), "nothing runs yet, so nothing to stop");
        let (built, sink) = watcher();
        job.run(move || Ok(built)).expect("a superseded arm isn't an error");

        assert!(!slot.is_running());
        assert_eq!(Arc::strong_count(&sink), 1, "the late watcher is still alive");
    }

    #[test]
    fn reopening_the_gate_mid_arm_keeps_only_the_newest_watcher() {
        let slot = fresh_slot();
        let first = slot.align(true).expect("a stopped slot arms");
        assert!(slot.align(false).is_none());
        let second = slot.align(true).expect("the reopened gate arms afresh");

        let (stale, stale_sink) = watcher();
        let (fresh, fresh_sink) = watcher();
        first.run(move || Ok(stale)).unwrap();
        second.run(move || Ok(fresh)).unwrap();

        assert_eq!(
            Arc::strong_count(&stale_sink),
            1,
            "the superseded watcher is still alive"
        );
        assert_eq!(Arc::strong_count(&fresh_sink), 2, "the newest watcher isn't installed");
        assert!(slot.is_running());
    }

    #[test]
    fn a_failed_arm_leaves_the_slot_ready_to_retry() {
        let slot = fresh_slot();
        let job = slot.align(true).expect("a stopped slot arms");

        assert!(
            job.run(|| Err(arm_error())).is_err(),
            "the arm's error reaches the caller"
        );
        assert!(slot.align(true).is_some(), "the next refresh retries");
    }

    /// A panicking arm, or a thread that never spawned, drops its ticket unfinished.
    #[test]
    fn a_lost_ticket_unwedges_the_slot() {
        let slot = fresh_slot();
        drop(slot.align(true).expect("a stopped slot arms"));

        assert!(slot.align(true).is_some(), "the slot stayed stuck in Arming");
    }

    #[test]
    fn closing_the_gate_while_running_hands_back_a_stop() {
        let slot = fresh_slot();
        let (running, sink) = watcher();
        slot.install(running);

        let job = slot.align(false).expect("a running watcher has to stop");
        assert!(!slot.is_running(), "the slot reads stopped as soon as the gate closes");
        job.run(|| unreachable!("a stop never arms")).unwrap();

        assert_eq!(Arc::strong_count(&sink), 1, "the stopped watcher is still alive");
    }
}
