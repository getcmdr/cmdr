//! Re-arming a volume's watcher after the OS ended its watch on its own
//! (`LiveLoopEnd::WatchLost`, #1363): a `ReadDirectoryChangesW` read error, the
//! watched root gone, or a watcher that couldn't start at all, while the volume stays
//! registered.
//!
//! Nothing watched the volume between the loss and the re-arm, and `notify` keeps no
//! journal, so the re-arm also catches up the way a launch would: a whole-watched
//! volume rescans, a branch-watched one watches its branches again with its rows
//! marked stale (`ensure_branch_watch(resuming)`). A drive that LEFT never waits here:
//! the host's removal notice (Windows' device notifications, task 39) stops its index,
//! which cancels this wait, and a replug is the user's to index again, as on macOS. So a
//! root that doesn't answer here is one still listed (a transient I/O error, a locked
//! BitLocker volume): its rows are marked stale and its root asked again on the next
//! backoff step (`watch/DETAILS.md` § "When the OS ends the watch").
//!
//! Each attempt first checks that the watch it replaces is still the lost one: a
//! "Rescan now" or a replugged drive during the wait brings up a healthy watcher, and
//! retiring it would leave the volume unwatched for the session.
//!
//! A watch that keeps dying would turn all this into a rescan loop, so re-arms back off
//! ([`RearmBackoff`]), timed by how long the lost watch's live loop ran. One timer per
//! wait, cancelled with the volume's work; nothing polled.

use std::time::Duration;

use super::super::state::{self, Handover};
use super::IndexManager;
use crate::indexing::lifecycle::rescan_request::ScanStartError;
use crate::indexing::writer::WriteMessage;

/// A watch whose live loop ran this long was healthy, so its loss starts the backoff
/// over.
const HEALTHY_AFTER: Duration = Duration::from_secs(10 * 60);
/// The wait before re-arming after a short-lived watch, doubled for each in a row.
const FIRST_DELAY: Duration = Duration::from_secs(30);
/// The longest a re-arm waits.
const MAX_DELAY: Duration = Duration::from_secs(30 * 60);

/// How many short-lived watches in a row this volume has lost. Lives on the manager,
/// so a volume's removal drops it.
// DEFAULT-OK: no loss yet is how every volume starts.
#[derive(Default)]
pub(in crate::indexing::lifecycle) struct RearmBackoff {
    short_lived: u32,
}

impl RearmBackoff {
    /// How long to wait before re-arming a watch whose live loop ran for `lived`.
    ///
    /// ⚠️ Timed from the loop's start, ❌ never from the last re-arm: on the rescan
    /// path the loop starts only once the scan ends, so a scan longer than
    /// [`HEALTHY_AFTER`] would make every loss look healthy, and a watch that died
    /// every time would rescan a big drive back to back.
    fn next_delay(&mut self, lived: Duration) -> Duration {
        if lived >= HEALTHY_AFTER {
            self.short_lived = 0;
            return Duration::ZERO;
        }
        self.short_lived = self.short_lived.saturating_add(1);
        FIRST_DELAY
            .saturating_mul(2u32.saturating_pow(self.short_lived - 1))
            .min(MAX_DELAY)
    }
}

/// What a re-arm does with a volume whose watch the OS ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rearm {
    /// Rescan through the scanner door, which starts a fresh watcher before it walks.
    Rescan,
    /// Watch the walk-covered branches again, rows marked stale.
    WatchBranches,
    /// The root doesn't answer: mark the rows stale and ask again later.
    WaitForTheDrive,
}

fn plan(branch_watched: bool, root_present: bool) -> Rearm {
    match (root_present, branch_watched) {
        (false, _) => Rearm::WaitForTheDrive,
        (true, true) => Rearm::WatchBranches,
        (true, false) => Rearm::Rescan,
    }
}

/// Whether the volume is still without a working watch, so a re-arm is still owed.
/// `watcher_ended` is the manager's current watcher's ended flag, `None` with no
/// watcher. A watcher that's up and running, or a walk that owns the volume (it starts
/// its own watcher, and its loop reports again if that one fails), means somebody
/// already re-armed it.
fn still_lost(watcher_ended: Option<bool>, walk_in_progress: bool) -> bool {
    !walk_in_progress && watcher_ended.is_none_or(|ended| ended)
}

/// How one attempt ended.
#[derive(Debug, PartialEq, Eq)]
enum Attempt {
    /// Re-armed, or nothing left to re-arm.
    Done,
    /// Not yet: wait the next backoff step and try again.
    TryAgain,
}

/// Re-arm `volume_id`'s watcher after the OS ended it, once its backoff allows, and
/// keep trying on the same backoff until it holds. Spawned by `after_live_loop`, never
/// awaited in place: it retires the loop that asked for it. Ends when the volume does
/// (its work is cancelled) or the runtime shuts down.
pub(in crate::indexing::lifecycle) async fn rearm_after_watch_lost(volume_id: String, lived: Duration) {
    let mut lived = lived;
    let mut tries = Tries::default();
    loop {
        let mut wait = None;
        state::with_running_manager(&volume_id, |mgr| {
            wait = Some((mgr.rearm_backoff.next_delay(lived), mgr.work.cancel.clone()));
        });
        let Some((delay, cancel)) = wait else {
            log::debug!("Watch re-arm: '{volume_id}' isn't running any more");
            return;
        };
        if !delay.is_zero() {
            log::warn!(
                "Watch re-arm: '{volume_id}' waits {} s before re-arming (its watch was short-lived)",
                delay.as_secs()
            );
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = cancel.cancelled() => return,
            }
        }
        match attempt(&volume_id, &mut tries) {
            Attempt::Done => return,
            Attempt::TryAgain => lived = Duration::ZERO,
        }
    }
}

/// What a run of attempts carries from one to the next.
// DEFAULT-OK: nothing learned before the first attempt.
#[derive(Default)]
struct Tries {
    /// Whether the lost watch was a branch watch, read at the first attempt.
    /// ⚠️ Kept, ❌ never re-read: retiring the watcher clears the manager's own
    /// `branch_watched`, so a second attempt would rescan a volume it should re-watch.
    branch_watched: Option<bool>,
    /// Whether this run already marked the rows stale.
    marked_stale: bool,
}

/// One try, with the manager held off the registry.
fn attempt(volume_id: &str, tries: &mut Tries) -> Attempt {
    let handover = state::off_the_registry(volume_id, "re-arming a watch the system ended", |mgr| {
        try_rearm(mgr, tries)
    });
    match handover {
        Err(e) => {
            log::debug!("Watch re-arm: '{volume_id}' found nothing running: {e}");
            Attempt::Done
        }
        Ok(Handover::Restored(attempt)) => attempt,
        Ok(Handover::TornDownWhileAway(_)) => {
            log::info!("Watch re-arm: '{volume_id}' was torn down meanwhile; the teardown ran instead");
            Attempt::Done
        }
    }
}

fn try_rearm(mgr: &mut IndexManager, tries: &mut Tries) -> Attempt {
    let volume_id = mgr.volume_id.clone();
    let watcher_ended = mgr
        .drive_watcher
        .as_ref()
        .map(|watcher| watcher.ended_flag().load(std::sync::atomic::Ordering::Acquire));
    let walk_in_progress = mgr.ground_in_flux.load(std::sync::atomic::Ordering::Relaxed) || mgr.phases_have_work();
    if !still_lost(watcher_ended, walk_in_progress) {
        log::info!("Watch re-arm: '{volume_id}' has a live watcher or a walk again; nothing to re-arm");
        return Attempt::Done;
    }
    // `is_dir` is false on ANY error (a locked BitLocker drive, a transient I/O
    // failure), which is why a miss asks again rather than giving up.
    let root_present = mgr.native_root.as_deref().is_some_and(std::path::Path::is_dir);
    let branch_watched = *tries.branch_watched.get_or_insert(mgr.branch_watched);
    let rearm = plan(branch_watched, root_present);
    // The dead watcher goes whatever comes next: a manager that still held it would
    // never start another (`plan_branch_watch`).
    if let Some(mut watcher) = mgr.retire_the_watcher() {
        watcher.stop();
    }
    log::info!("Watch re-arm: '{volume_id}' {rearm:?}");
    match rearm {
        Rearm::Rescan => match mgr.cover_or_scan("watch lost") {
            Ok(()) | Err(ScanStartError::AlreadyScanning) => Attempt::Done,
            Err(e) => {
                log::warn!("Watch re-arm: '{volume_id}' couldn't start its rescan yet ({e}); trying again later");
                Attempt::TryAgain
            }
        },
        Rearm::WatchBranches => {
            mgr.ensure_branch_watch(true);
            if mgr.drive_watcher.is_some() {
                Attempt::Done
            } else {
                Attempt::TryAgain
            }
        }
        Rearm::WaitForTheDrive => {
            // Nothing watches these rows until the root answers, so they render stale
            // rather than current. Once per wait: a second bump marks nothing new.
            if !std::mem::replace(&mut tries.marked_stale, true) {
                let _ = mgr.writer.send(WriteMessage::BumpCurrentEpoch);
            }
            Attempt::TryAgain
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_that_lived_long_re_arms_at_once() {
        let mut backoff = RearmBackoff::default();
        assert_eq!(backoff.next_delay(HEALTHY_AFTER), Duration::ZERO);
    }

    /// A watch that keeps dying must not become a rescan loop.
    #[test]
    fn a_watch_that_keeps_dying_backs_off_doubling_up_to_the_cap() {
        let mut backoff = RearmBackoff::default();
        let delays: Vec<u64> = (0..8)
            .map(|_| backoff.next_delay(Duration::from_secs(1)).as_secs())
            .collect();
        assert_eq!(delays, [30, 60, 120, 240, 480, 960, 1800, 1800]);
    }

    #[test]
    fn a_watch_that_lived_long_starts_the_backoff_over() {
        let mut backoff = RearmBackoff::default();
        backoff.next_delay(Duration::ZERO);
        backoff.next_delay(Duration::ZERO);
        assert_eq!(backoff.next_delay(HEALTHY_AFTER), Duration::ZERO);
        assert_eq!(backoff.next_delay(Duration::ZERO), FIRST_DELAY);
    }

    /// Review of #1456: health was timed from the last re-arm, and on the rescan path
    /// the loss shows only once the scan ends, so a drive whose scan took longer than
    /// the healthy window re-armed with no wait, every time. The loop's own run time
    /// is what counts: a watch that died at once backs off however long the scan took.
    #[test]
    fn a_long_scan_does_not_make_a_watch_that_died_at_once_look_healthy() {
        let mut backoff = RearmBackoff::default();
        let loop_ran = Duration::ZERO;
        assert_eq!(backoff.next_delay(loop_ran), FIRST_DELAY);
        assert_eq!(backoff.next_delay(loop_ran), FIRST_DELAY * 2);
    }

    #[test]
    fn a_re_arm_rescans_watches_branches_or_waits_for_the_drive() {
        assert_eq!(plan(false, true), Rearm::Rescan);
        assert_eq!(plan(true, true), Rearm::WatchBranches);
        assert_eq!(plan(false, false), Rearm::WaitForTheDrive);
        assert_eq!(plan(true, false), Rearm::WaitForTheDrive);
    }

    /// Review of #1456, case (a): "Rescan now" during the wait started a scan with a
    /// fresh watcher. Re-arming would retire that watcher mid-scan, and the scan's
    /// loop would then end as a plain stop: no watch for the session.
    #[test]
    fn a_rescan_started_during_the_wait_is_left_alone() {
        assert!(
            !still_lost(Some(false), true),
            "a walk owns the volume, with its own watcher"
        );
        assert!(
            !still_lost(None, true),
            "and one whose watcher failed reports through its own loop"
        );
    }

    /// Case (b): a drive pulled and plugged back during the wait came up with a healthy
    /// watcher. Re-arming would retire it and rescan the drive for nothing.
    #[test]
    fn a_healthy_watcher_that_came_up_during_the_wait_is_left_alone() {
        assert!(!still_lost(Some(false), false));
    }

    #[test]
    fn the_lost_watcher_or_none_at_all_is_still_owed_a_re_arm() {
        assert!(still_lost(Some(true), false), "the dead watcher is still installed");
        assert!(still_lost(None, false), "a previous attempt already retired it");
    }
}
