//! Re-arming a volume's watcher after the OS ended its watch on its own
//! (`LiveLoopEnd::WatchLost`, #1363): a `ReadDirectoryChangesW` read error, or the
//! watched root gone, while the volume stays registered.
//!
//! Nothing watched the volume between the loss and the re-arm, and `notify` keeps no
//! journal, so the re-arm also catches up the way a launch would: a whole-watched
//! volume rescans, a branch-watched one watches its branches again with its rows
//! marked stale (`ensure_branch_watch(resuming)`). A volume whose root is gone gets
//! neither: its drive was pulled, and its removal tears the index down; a rescan would
//! only find every row missing.
//!
//! A watch that keeps dying would turn that into a rescan loop, so re-arms back off
//! ([`RearmBackoff`]): the first one is immediate, and each that follows a short-lived
//! watch waits twice as long as the last, up to [`MAX_DELAY`]. One timer per wait,
//! nothing polled.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::super::state::{self, Handover};
use cmdr_fs::ignore_poison::IgnorePoison;

/// A watch that lived this long was healthy, so its loss starts the backoff over.
const HEALTHY_AFTER: Duration = Duration::from_secs(10 * 60);
/// The wait before the first re-arm that follows a short-lived watch.
const FIRST_DELAY: Duration = Duration::from_secs(30);
/// The longest a re-arm waits.
const MAX_DELAY: Duration = Duration::from_secs(30 * 60);

/// One volume's re-arm history: when the last re-arm ran, and how many short-lived
/// watches in a row came before it.
// DEFAULT-OK: no re-arm yet is how every volume starts.
#[derive(Default)]
struct RearmBackoff {
    last_rearm: Option<Instant>,
    short_lived: u32,
}

impl RearmBackoff {
    /// How long to wait before re-arming a watch lost at `now`, recording the re-arm.
    fn next_delay(&mut self, now: Instant) -> Duration {
        let healthy = self
            .last_rearm
            .is_none_or(|last| now.saturating_duration_since(last) >= HEALTHY_AFTER);
        let delay = if healthy {
            self.short_lived = 0;
            Duration::ZERO
        } else {
            self.short_lived = self.short_lived.saturating_add(1);
            FIRST_DELAY
                .saturating_mul(2u32.saturating_pow(self.short_lived - 1))
                .min(MAX_DELAY)
        };
        self.last_rearm = Some(now + delay);
        delay
    }
}

/// Every volume's re-arm history. Outlives the managers on purpose: a drive that is
/// stopped and started again in a loop still backs off.
static BACKOFF: Mutex<Option<HashMap<String, RearmBackoff>>> = Mutex::new(None);

fn next_delay(volume_id: &str, now: Instant) -> Duration {
    BACKOFF
        .lock_ignore_poison()
        .get_or_insert_with(HashMap::new)
        .entry(volume_id.to_string())
        .or_default()
        .next_delay(now)
}

/// What a re-arm does with a volume whose watch the OS ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rearm {
    /// Rescan through the scanner door, which starts a fresh watcher before it walks.
    Rescan,
    /// Watch the walk-covered branches again, rows marked stale.
    WatchBranches,
    /// The root is gone: leave it to the volume's removal.
    WaitForTheDrive,
}

fn plan(branch_watched: bool, root_present: bool) -> Rearm {
    match (root_present, branch_watched) {
        (false, _) => Rearm::WaitForTheDrive,
        (true, true) => Rearm::WatchBranches,
        (true, false) => Rearm::Rescan,
    }
}

/// Re-arm `volume_id`'s watcher after the OS ended it, once its backoff allows.
/// Spawned by `after_live_loop`, never awaited in place: it retires the loop that
/// asked for it.
pub(in crate::indexing::lifecycle) async fn rearm_after_watch_lost(volume_id: String) {
    let delay = next_delay(&volume_id, Instant::now());
    if !delay.is_zero() {
        log::warn!(
            "Watch re-arm: '{volume_id}' lost its watch again soon after the last re-arm; waiting {} s",
            delay.as_secs()
        );
        tokio::time::sleep(delay).await;
    }
    let handover = state::off_the_registry(&volume_id, "re-arming a watch the system ended", |mgr| {
        let root_present = mgr.native_root.as_deref().is_some_and(std::path::Path::is_dir);
        let rearm = plan(mgr.branch_watched, root_present);
        // The dead watcher goes whatever comes next: its loop already ended, and a
        // manager that still held it would never start another (`plan_branch_watch`).
        if let Some(mut watcher) = mgr.retire_the_watcher() {
            watcher.stop();
        }
        log::info!("Watch re-arm: '{volume_id}' {rearm:?}");
        match rearm {
            Rearm::Rescan => {
                if let Err(ref e) = mgr.cover_or_scan("watch lost") {
                    log::warn!("Watch re-arm: '{volume_id}' couldn't start its rescan: {e}");
                }
            }
            Rearm::WatchBranches => mgr.ensure_branch_watch(true),
            Rearm::WaitForTheDrive => {}
        }
    });
    match handover {
        Err(e) => log::debug!("Watch re-arm: '{volume_id}' found nothing running: {e}"),
        Ok(Handover::Restored(())) => {}
        Ok(Handover::TornDownWhileAway(())) => {
            log::info!("Watch re-arm: '{volume_id}' was torn down meanwhile; the teardown ran instead");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_loss_re_arms_at_once() {
        let mut backoff = RearmBackoff::default();
        assert_eq!(backoff.next_delay(Instant::now()), Duration::ZERO);
    }

    /// A watch that keeps dying must not become a rescan loop.
    #[test]
    fn a_watch_that_keeps_dying_backs_off_doubling_up_to_the_cap() {
        let mut backoff = RearmBackoff::default();
        let mut now = Instant::now();
        assert_eq!(backoff.next_delay(now), Duration::ZERO);
        let mut delays = Vec::new();
        for _ in 0..8 {
            now += Duration::from_secs(1);
            let delay = backoff.next_delay(now);
            delays.push(delay.as_secs());
            now += delay;
        }
        assert_eq!(delays, [30, 60, 120, 240, 480, 960, 1800, 1800]);
    }

    #[test]
    fn a_watch_that_lived_long_starts_the_backoff_over() {
        let mut backoff = RearmBackoff::default();
        let now = Instant::now();
        backoff.next_delay(now);
        assert_eq!(backoff.next_delay(now + Duration::from_secs(1)), FIRST_DELAY);
        let later = now + FIRST_DELAY + HEALTHY_AFTER + Duration::from_secs(1);
        assert_eq!(backoff.next_delay(later), Duration::ZERO);
    }

    #[test]
    fn a_re_arm_rescans_watches_branches_or_waits_for_the_drive() {
        assert_eq!(plan(false, true), Rearm::Rescan);
        assert_eq!(plan(true, true), Rearm::WatchBranches);
        assert_eq!(plan(false, false), Rearm::WaitForTheDrive);
        assert_eq!(plan(true, false), Rearm::WaitForTheDrive);
    }
}
