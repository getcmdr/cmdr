//! The bounded real-place fetch: a slow place costs only its own icon, lands in the cache
//! later, isn't asked twice while in flight, and the in-flight count stays capped.

use std::sync::Mutex;
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;

use super::{IN_FLIGHT, MAX_IN_FLIGHT, fetch_within_using};
use crate::icons::memory_cache::{epoch, get_cached_icon};
use crate::test_support::wait_until;

/// Held by a test while its "slow" places must not answer.
static SLOW_GATE: Mutex<()> = Mutex::new(());

/// Answers `url-{path}`, after `SLOW_GATE` opens for a path starting with `slow`.
fn gated_fetch(path: &str) -> Option<String> {
    if path.starts_with("slow") {
        drop(SLOW_GATE.lock_ignore_poison());
    }
    Some(format!("url-{path}"))
}

fn item(id: &str, path: &str) -> (String, String) {
    (id.to_string(), path.to_string())
}

#[test]
fn a_slow_place_costs_only_its_own_icon_and_lands_in_the_cache_later() {
    let gate = SLOW_GATE.lock_ignore_poison();
    let items = vec![
        item("path:ppf-test/slow", "slow-1"),
        item("path:ppf-test/fast", "fast-1"),
    ];

    let landed = fetch_within_using(items, epoch(), Duration::from_millis(300), gated_fetch);
    assert_eq!(
        landed,
        vec![("path:ppf-test/fast".to_string(), "url-fast-1".to_string())]
    );

    // Still in flight: asked again, it starts no second lookup and answers nothing.
    let again = fetch_within_using(
        vec![item("path:ppf-test/slow", "slow-1")],
        epoch(),
        Duration::ZERO,
        gated_fetch,
    );
    assert!(again.is_empty());
    assert!(IN_FLIGHT.lock_ignore_poison().contains("path:ppf-test/slow"));

    drop(gate);
    wait_until(
        Duration::from_secs(5),
        "the slow place's icon lands in the cache",
        || get_cached_icon("path:ppf-test/slow").as_deref() == Some("url-slow-1"),
    );
    wait_until(
        Duration::from_secs(5),
        "the slow place leaves the in-flight set",
        || !IN_FLIGHT.lock_ignore_poison().contains("path:ppf-test/slow"),
    );
}

#[test]
fn stuck_lookups_never_exceed_the_cap() {
    let gate = SLOW_GATE.lock_ignore_poison();
    let items = (0..MAX_IN_FLIGHT + 4)
        .map(|n| item(&format!("path:ppf-cap/{n}"), &format!("slow-cap-{n}")))
        .collect();

    let landed = fetch_within_using(items, epoch(), Duration::from_millis(50), gated_fetch);
    assert!(landed.is_empty());
    assert_eq!(IN_FLIGHT.lock_ignore_poison().len(), MAX_IN_FLIGHT);

    drop(gate);
    wait_until(Duration::from_secs(5), "every stuck lookup finishes", || {
        IN_FLIGHT.lock_ignore_poison().is_empty()
    });
}
