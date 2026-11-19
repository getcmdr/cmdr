//! The discovery state on every OS, with discovery played by a thread the test controls.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::*;

/// Long enough that a discovery the test answers at once never runs into it.
const PATIENT: Duration = Duration::from_secs(30);
const IMPATIENT: Duration = Duration::from_millis(50);
const PAC: &str = "http://wpad.corp.test/wpad.dat";

/// A [`Wpad`] whose discoveries the test counts and answers by hand.
struct Harness {
    wpad: Arc<Wpad>,
    started: Arc<AtomicUsize>,
}

impl Harness {
    fn new(retry_after: Duration) -> Self {
        Self {
            wpad: Arc::new(Wpad::new(retry_after)),
            started: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Asks on `network`, a discovery (if one starts) answering `answer` after `delay`.
    fn ask(&self, network: u64, wait: Duration, answer: Option<&'static str>, delay: Duration) -> Option<String> {
        let (wpad, started) = (self.wpad.clone(), self.started.clone());
        self.wpad.pac_url(network, wait, move || {
            started.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                // allowed-test-sleep: a discovery's own duration is the subject
                std::thread::sleep(delay);
                wpad.finish(network, answer.map(String::from));
            });
        })
    }

    fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }
}

#[test]
fn the_first_ask_starts_a_discovery_and_waits_for_its_answer() {
    let harness = Harness::new(PATIENT);
    assert_eq!(harness.ask(1, PATIENT, Some(PAC), Duration::ZERO), Some(PAC.into()));
    assert_eq!(harness.started(), 1);
}

/// Once per network: every later ask on it reads the answer, with no discovery and no wait.
#[test]
fn a_network_is_discovered_once() {
    let harness = Harness::new(PATIENT);
    assert_eq!(harness.ask(1, PATIENT, Some(PAC), Duration::ZERO), Some(PAC.into()));
    let started = Instant::now();
    assert_eq!(
        harness.ask(1, PATIENT, Some("http://other.test/x.pac"), Duration::ZERO),
        Some(PAC.into())
    );
    assert!(started.elapsed() < IMPATIENT, "took {:?}", started.elapsed());
    assert_eq!(harness.started(), 1);
}

/// Asks arriving together share the one discovery: nothing queues behind a lock, and nothing
/// discovers twice.
#[test]
fn asks_during_a_discovery_wait_for_it_rather_than_starting_another() {
    let harness = Arc::new(Harness::new(PATIENT));
    let asks: Vec<_> = (0..4)
        .map(|_| {
            let harness = harness.clone();
            std::thread::spawn(move || harness.ask(1, PATIENT, Some(PAC), Duration::from_millis(200)))
        })
        .collect();
    for ask in asks {
        assert_eq!(ask.join().expect("an asking thread"), Some(PAC.into()));
    }
    assert_eq!(harness.started(), 1);
}

/// A slow discovery (a stalling WPAD server) costs an ask at most its wait, and the answer is
/// kept for the asks after it, still from that one discovery.
#[test]
fn a_slow_discovery_costs_an_ask_only_its_wait() {
    let harness = Harness::new(PATIENT);
    let started = Instant::now();
    assert_eq!(harness.ask(1, IMPATIENT, Some(PAC), Duration::from_millis(400)), None);
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(harness.ask(1, PATIENT, Some(PAC), Duration::ZERO), Some(PAC.into()));
    assert_eq!(harness.started(), 1);
}

/// No WPAD on this network: asks skip it at once, and discovery isn't rerun until the retry
/// interval has passed.
#[test]
fn a_network_without_wpad_is_skipped_until_the_retry() {
    let harness = Harness::new(PATIENT);
    assert_eq!(harness.ask(1, PATIENT, None, Duration::ZERO), None);
    assert_eq!(harness.ask(1, PATIENT, None, Duration::ZERO), None);
    assert_eq!(harness.started(), 1);
}

/// Past the retry interval, a failed discovery reruns in the background: the ask that starts it
/// doesn't wait for it, and a server that came back is found for the asks after.
#[test]
fn a_failed_discovery_reruns_in_the_background_after_the_retry_interval() {
    let harness = Harness::new(Duration::ZERO);
    assert_eq!(harness.ask(1, PATIENT, None, Duration::ZERO), None);
    let started = Instant::now();
    assert_eq!(harness.ask(1, PATIENT, Some(PAC), Duration::from_millis(100)), None);
    assert!(started.elapsed() < IMPATIENT, "took {:?}", started.elapsed());
    assert_eq!(harness.started(), 2);
    let deadline = Instant::now() + PATIENT;
    while harness.ask(1, Duration::ZERO, Some(PAC), Duration::ZERO) != Some(PAC.into()) {
        assert!(Instant::now() < deadline, "the rerun never landed");
        std::thread::yield_now();
    }
}

/// A new network starts over: the old network's answer is dropped, and a discovery that was
/// still running for it can't land on the new one.
#[test]
fn a_new_network_starts_over() {
    let harness = Harness::new(PATIENT);
    assert_eq!(harness.ask(1, IMPATIENT, Some(PAC), Duration::from_millis(300)), None);
    assert_eq!(harness.ask(2, PATIENT, None, Duration::ZERO), None);
    assert_eq!(harness.started(), 2);
    // The first network's discovery lands now, late; the second network keeps its own answer.
    // allowed-test-sleep: outliving the stale discovery is the subject
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(harness.ask(2, PATIENT, Some(PAC), Duration::ZERO), None);
    assert_eq!(harness.started(), 2);
}

/// An ask about a network the machine has already left gets nothing and starts nothing.
#[test]
fn an_ask_about_an_older_network_gets_nothing() {
    let harness = Harness::new(PATIENT);
    assert_eq!(harness.ask(2, PATIENT, Some(PAC), Duration::ZERO), Some(PAC.into()));
    assert_eq!(harness.ask(1, PATIENT, Some(PAC), Duration::ZERO), None);
    assert_eq!(harness.started(), 1);
}
