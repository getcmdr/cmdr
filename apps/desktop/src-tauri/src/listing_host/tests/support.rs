//! Probes on the test binary's ONE engine (`listing_host::test_listings`): a
//! lifecycle observer that records what the cache and the FSEvents watcher held
//! when it was told, and a probe that records what the cache held when a watch
//! disarmed.
//!
//! The engine is shared by every test in the binary, so each probe answers only
//! for the listing ids a test opted into; every other test's listings pass
//! straight through.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use cmdr_fs::ignore_poison::{IgnorePoison, RwLockIgnorePoison};

use crate::listing_host::listings;
use cmdr_listing::ListingId;
use cmdr_listing::host::{ListingObserver, OpenedListing};

/// One lifecycle call as the probe heard it, with whether the engine held the
/// listing at that moment (and, on a close, whether its FSEvents watch was
/// still armed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Heard {
    Opened { cached: bool },
    Closed { cached: bool, watching: bool },
}

/// Listing id → what the probe heard for it, in order.
static HEARD: LazyLock<Mutex<HashMap<String, Vec<Heard>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Listing ids a test opted into.
static WATCHED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// The lifecycle probe, the last observer of the test binary's engine.
pub(crate) fn lifecycle_probe() -> Arc<dyn ListingObserver> {
    Arc::new(LifecycleProbe)
}

struct LifecycleProbe;

impl LifecycleProbe {
    fn hear(listing_id: &ListingId, heard: impl FnOnce(bool) -> Heard) {
        if !WATCHED.lock_ignore_poison().contains(listing_id.as_str()) {
            return;
        }
        let cached = listings().contains(listing_id);
        HEARD
            .lock_ignore_poison()
            .entry(listing_id.to_string())
            .or_default()
            .push(heard(cached));
    }
}

impl ListingObserver for LifecycleProbe {
    fn id(&self) -> &'static str {
        "lifecycle-probe"
    }

    fn listing_opened(&self, opened: &OpenedListing<'_>) {
        Self::hear(opened.id, |cached| Heard::Opened { cached });
    }

    fn listing_closed(&self, id: &ListingId) {
        let watching = crate::file_system::watcher::WATCHER_MANAGER
            .read_ignore_poison()
            .watches
            .contains_key(id.as_str());
        Self::hear(id, |cached| Heard::Closed { cached, watching });
    }
}

/// Makes the lifecycle probe listen for `listing_id`.
#[cfg(unix)]
pub(crate) fn record_lifecycle_of(listing_id: &str) {
    WATCHED.lock_ignore_poison().insert(listing_id.to_string());
}

/// What the lifecycle probe heard for `listing_id`, in order.
#[cfg(unix)]
pub(crate) fn heard(listing_id: &str) -> Vec<Heard> {
    HEARD.lock_ignore_poison().get(listing_id).cloned().unwrap_or_default()
}

/// Listing ids whose watch disarms a test records, and per id whether the
/// engine still held the listing at each disarm.
static DISARMS: LazyLock<Mutex<HashMap<String, Vec<bool>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Makes the disarm probe record `listing_id`'s watch disarms.
#[cfg(unix)]
pub(crate) fn record_disarms_of(listing_id: &str) {
    DISARMS.lock_ignore_poison().insert(listing_id.to_string(), Vec::new());
}

/// Called by `watcher::stop_watching`: notes whether the engine still held the
/// listing when its watch was disarmed, for listings a test opted into.
pub(crate) fn note_disarm(listing_id: &str) {
    if !DISARMS.lock_ignore_poison().contains_key(listing_id) {
        return;
    }
    let cached = listings().contains(&ListingId::new(listing_id));
    if let Some(seen) = DISARMS.lock_ignore_poison().get_mut(listing_id) {
        seen.push(cached);
    }
}

/// Per disarm of `listing_id`'s watch, whether the engine still held the listing.
#[cfg(unix)]
pub(crate) fn disarms_of(listing_id: &str) -> Vec<bool> {
    DISARMS
        .lock_ignore_poison()
        .get(listing_id)
        .cloned()
        .unwrap_or_default()
}
