//! Real-place icon fetches (`special:*`, `pkg:*`, `path:*`), each on its own detached thread
//! with a bounded wait, so one slow place costs only its own icon.
//!
//! `get_icons` runs under the IPC command's 2 s deadline, and a timed-out call answers nothing
//! at all. A place's lookup can hang for as long as a dead share takes to fail, so it never
//! holds up the batch: `get_icons` resolves the type ids first, then waits here only up to
//! what's left of its budget. A fetch that outlives the wait keeps running and caches its icon
//! when it lands, so the next request hits it. At most [`MAX_IN_FLIGHT`] run at once and a
//! place already in flight isn't asked again, so stuck threads can't pile up with every visit.

use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use cmdr_fs::icons::own_icons;
use cmdr_fs::ignore_poison::IgnorePoison;

use super::disk_cache;
use super::memory_cache::{CacheEpoch, cache_icon};

/// Real-place lookups running at once, across every caller.
pub(super) const MAX_IN_FLIGHT: usize = 16;

/// The icon ids whose lookup is running right now.
static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Fetches each `(icon_id, real_path)` through the platform, waiting at most `wait`, and
/// answers the ones that landed in time. Every landed icon is cached (memory, plus the disk
/// tier where `own_icons::keeps_on_disk` allows) whether or not it beat the wait.
pub(super) fn fetch_within(items: Vec<(String, String)>, epoch: CacheEpoch, wait: Duration) -> Vec<(String, String)> {
    fetch_within_using(items, epoch, wait, |path| {
        super::platform::fetch_path_icons(vec![path.to_string()])
            .pop()
            .and_then(|(_, url)| url)
    })
}

/// [`fetch_within`] with the per-place lookup injected, for tests.
fn fetch_within_using(
    items: Vec<(String, String)>,
    epoch: CacheEpoch,
    wait: Duration,
    fetch: fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let deadline = Instant::now() + wait;
    let (sender, receiver) = mpsc::channel();
    let mut started = 0usize;
    for (icon_id, real_path) in items {
        if !claim(&icon_id) {
            continue;
        }
        let sender = sender.clone();
        let id = icon_id.clone();
        let spawned = std::thread::Builder::new()
            .name("icon_place_fetch".into())
            .spawn(move || {
                let url = fetch(&real_path);
                if let Some(url) = &url
                    && cache_icon(epoch, id.clone(), url.clone())
                    && own_icons::keeps_on_disk(&id)
                {
                    disk_cache::store(&id, &real_path, epoch.variant, url);
                }
                IN_FLIGHT.lock_ignore_poison().remove(&id);
                // The receiver is gone once the caller stopped waiting: the icon is cached anyway.
                let _ = sender.send((id, url));
            });
        match spawned {
            Ok(_) => started += 1,
            Err(e) => {
                IN_FLIGHT.lock_ignore_poison().remove(&icon_id);
                log::warn!(target: "icons", "Couldn't start a place icon lookup: {e}");
            }
        }
    }
    drop(sender);

    let mut landed = Vec::with_capacity(started);
    for _ in 0..started {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((icon_id, Some(url))) => landed.push((icon_id, url)),
            Ok((_, None)) => {}
            Err(_) => break,
        }
    }
    landed
}

/// Marks `icon_id` in flight, or answers false when it already is or the cap is reached.
fn claim(icon_id: &str) -> bool {
    let mut in_flight = IN_FLIGHT.lock_ignore_poison();
    if in_flight.len() >= MAX_IN_FLIGHT || in_flight.contains(icon_id) {
        return false;
    }
    in_flight.insert(icon_id.to_string())
}

#[cfg(test)]
#[path = "per_place_fetch_test.rs"]
mod tests;
