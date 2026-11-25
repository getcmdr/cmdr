use super::disk::*;
use super::*;
use crate::test_support::wait_until_async;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn explode() -> Result<EjectOutcome, EjectError> {
    panic!("teardown blew up")
}

#[tokio::test]
async fn a_second_eject_joins_the_one_in_flight_and_gets_its_answer() {
    let vid = "volumes-cmdr-test-joins-in-flight";
    let runs = Arc::new(AtomicUsize::new(0));
    let (release, held) = tokio::sync::oneshot::channel::<()>();

    let first = join_or_start(vid, {
        let runs = Arc::clone(&runs);
        move || async move {
            runs.fetch_add(1, Ordering::SeqCst);
            let _ = held.await;
            Err(EjectError::UnmountRefused {
                holders: super::super::holders::HolderScan::not_scanned(),
                detail: "held by sleep".to_string(),
            })
        }
    });
    assert!(
        ejecting_volume_ids().contains(&vid.to_string()),
        "the volume reads as ejecting while its flight runs"
    );
    wait_until_async(Duration::from_secs(5), "the first teardown to start", || {
        runs.load(Ordering::SeqCst) == 1
    })
    .await;

    // Arrives while the first is still held: it must join, not run its own.
    let second = join_or_start(vid, {
        let runs = Arc::clone(&runs);
        move || async move {
            runs.fetch_add(1, Ordering::SeqCst);
            Ok(EjectOutcome::Unmounted)
        }
    });

    let _ = release.send(());
    let (first, second) = tokio::join!(first, second);

    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "a joined request must not run a second teardown"
    );
    for result in [first, second] {
        assert!(
            matches!(result, Err(EjectError::UnmountRefused { ref detail, .. }) if detail == "held by sleep"),
            "both callers get the one flight's answer, got {result:?}"
        );
    }
    assert!(
        !ejecting_volume_ids().contains(&vid.to_string()),
        "the set clears once the flight lands"
    );
}

#[tokio::test]
async fn a_request_after_the_flight_landed_starts_a_new_one() {
    // A retry after a refusal really retries: nothing caches the old answer.
    let vid = "volumes-cmdr-test-fresh-flight";
    let first = join_or_start(vid, || async { Err(EjectError::TimedOut) }).await;
    assert!(matches!(first, Err(EjectError::TimedOut)), "got {first:?}");

    let second = join_or_start(vid, || async { Ok(EjectOutcome::Unmounted) }).await;
    assert!(second.is_ok(), "got {second:?}");
}

// ── One flight per physical disk ──────────────────────────────────

/// Runs `body` as a flight of `volume_id`, so it can own its disk the way
/// `eject_now` does: `join_or_own_disk` reads the flight from the ejecting set.
fn as_a_flight<F, Fut>(volume_id: &str, body: F) -> Flight
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<EjectOutcome, EjectError>> + Send + 'static,
{
    join_or_start(volume_id, body)
}

fn owned(flight: DiskFlight) -> DiskOwnership {
    match flight {
        DiskFlight::Owner(ownership) => ownership,
        DiskFlight::Joined(_) | DiskFlight::DecideAgainAfter(_) => panic!("expected to own the disk"),
    }
}

#[tokio::test]
async fn a_disk_flight_takes_its_siblings_into_the_ejecting_set_so_their_own_eject_joins_it() {
    // A whose disk also carries B: B's eject must join A's flight in
    // `join_or_start`, BEFORE `is_already_unmounted` or any teardown of its own.
    let (a, b) = ("volumes-cmdr-test-disk-a", "volumes-cmdr-test-disk-b");
    let (release, held) = tokio::sync::oneshot::channel::<()>();
    let (owned_disk, disk_is_owned) = tokio::sync::oneshot::channel::<()>();
    let ran_alone = Arc::new(AtomicUsize::new(0));

    let first = as_a_flight(a, {
        let ran_alone = Arc::clone(&ran_alone);
        move || async move {
            ran_alone.fetch_add(1, Ordering::SeqCst);
            let ownership = owned(join_or_own_disk(DiskKey(4242), a, &[a.to_string(), b.to_string()]));
            assert_eq!(ownership.adopted(), [b.to_string()], "the sibling joined this flight");
            let _ = owned_disk.send(());
            let _ = held.await;
            drop(ownership);
            Err(EjectError::UnmountRefused {
                holders: super::super::holders::HolderScan::not_scanned(),
                detail: "held by sleep".to_string(),
            })
        }
    });
    let _ = disk_is_owned.await;
    assert!(
        ejecting_volume_ids().contains(&b.to_string()),
        "an adopted sibling reads as ejecting, so no start lands on it"
    );

    let second = join_or_start(b, {
        let ran_alone = Arc::clone(&ran_alone);
        move || async move {
            ran_alone.fetch_add(1, Ordering::SeqCst);
            Ok(EjectOutcome::Unmounted)
        }
    });

    let _ = release.send(());
    let (first, second) = tokio::join!(first, second);
    assert_eq!(ran_alone.load(Ordering::SeqCst), 1, "one teardown for the whole disk");
    for result in [first, second] {
        assert!(
            matches!(result, Err(EjectError::UnmountRefused { .. })),
            "both volumes get the disk flight's answer, got {result:?}"
        );
    }
    assert!(
        !ejecting_volume_ids().contains(&b.to_string()),
        "the adopted sibling leaves the set when the flight lands"
    );
}

// ── Holding past a timeout (Windows' drive flight) ─────────────────

#[tokio::test]
async fn a_flight_holding_past_its_landing_keeps_its_unit_ejecting_and_a_second_eject_joins_it() {
    // Windows: `CM_Request_Device_EjectW` timed out but may still land. The flight answers at
    // once, yet E: (asked) and F: (its sibling partition) stay ejecting until the request truly
    // ends, so no operation starts on them and a second eject joins instead of tearing down.
    let (e, f) = ("volumes-cmdr-test-hold-e", "volumes-cmdr-test-hold-f");
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let first = join_or_start(e, move || async move {
        let mut ownership = owned(join_or_own_disk(DiskKey(7070), e, &[e.to_string(), f.to_string()]));
        ownership.hold_past_landing(e);
        let _ = held_tx.send(ownership);
        Err(EjectError::TimedOut)
    });
    assert!(matches!(first.await, Err(EjectError::TimedOut)));
    let ownership = held_rx.recv().expect("the flight handed its hold over");
    assert!(
        is_ejecting(e) && is_ejecting(f),
        "the unit stays ejecting past the answer"
    );

    let runs = Arc::new(AtomicUsize::new(0));
    let second = join_or_start(f, {
        let runs = Arc::clone(&runs);
        move || async move {
            runs.fetch_add(1, Ordering::SeqCst);
            Ok(EjectOutcome::DiskEjected)
        }
    });
    assert!(
        matches!(second.await, Err(EjectError::TimedOut)),
        "the second eject joined the first and heard its answer"
    );
    assert_eq!(
        runs.load(Ordering::SeqCst),
        0,
        "no second teardown under the first request"
    );

    drop(ownership);
    assert!(!is_ejecting(e) && !is_ejecting(f), "the request's end ends the hold");
}

#[tokio::test]
async fn a_sibling_whose_own_flight_started_first_awaits_the_disk_flight_instead_of_tearing_down_again() {
    // The other order: B's own eject is already running when A resolves the same
    // disk. B keeps its flight, A owns the disk, and B awaits A's teardown.
    let (a, b) = ("volumes-cmdr-test-order-a", "volumes-cmdr-test-order-b");
    let key = DiskKey(9977);
    let (b_owns, b_owned_it) = tokio::sync::oneshot::channel::<()>();
    let (release, held) = tokio::sync::oneshot::channel::<()>();
    let teardowns = Arc::new(AtomicUsize::new(0));

    let b_flight = as_a_flight(b, {
        let teardowns = Arc::clone(&teardowns);
        move || async move {
            let ownership = owned(join_or_own_disk(key, b, &[b.to_string()]));
            let _ = b_owns.send(());
            let _ = held.await;
            teardowns.fetch_add(1, Ordering::SeqCst);
            drop(ownership);
            Err(EjectError::TimedOut)
        }
    });
    let _ = b_owned_it.await;

    let a_flight = as_a_flight(a, {
        let teardowns = Arc::clone(&teardowns);
        move || async move {
            match join_or_own_disk(key, a, &[a.to_string(), b.to_string()]) {
                DiskFlight::Joined(running) => running.await,
                DiskFlight::DecideAgainAfter(_) => panic!("B's flight takes the whole disk down"),
                DiskFlight::Owner(_) => {
                    teardowns.fetch_add(1, Ordering::SeqCst);
                    panic!("B's flight already owns this disk")
                }
            }
        }
    });

    let _ = release.send(());
    let (a_result, b_result) = tokio::join!(a_flight, b_flight);
    assert_eq!(teardowns.load(Ordering::SeqCst), 1, "one teardown for the disk");
    assert!(matches!(a_result, Err(EjectError::TimedOut)), "got {a_result:?}");
    assert!(matches!(b_result, Err(EjectError::TimedOut)), "got {b_result:?}");
}

#[tokio::test]
async fn a_flight_reads_its_own_hold_on_a_sibling_apart_from_a_later_flights() {
    // What a flight asks before it hands a sibling's index back: its own adopted
    // ids are ejecting until it lands, and refusing over that would strand them.
    let (a, b) = ("volumes-cmdr-test-owner-a", "volumes-cmdr-test-owner-b");
    let (done, is_done) = tokio::sync::oneshot::channel::<()>();
    let (release, held) = tokio::sync::oneshot::channel::<()>();

    let flight = as_a_flight(a, move || async move {
        let ownership = owned(join_or_own_disk(DiskKey(311), a, &[a.to_string(), b.to_string()]));
        assert!(
            !is_ejecting_by_another(b, ownership.flight_id()),
            "the flight's own adopted sibling is not someone else's eject"
        );
        assert!(
            is_ejecting_by_another(b, ownership.flight_id() + 1_000),
            "another flight's hold on the sibling is"
        );
        let _ = done.send(());
        let _ = held.await;
        drop(ownership);
        Ok(EjectOutcome::Unmounted)
    });
    let _ = is_done.await;
    let _ = release.send(());
    assert!(flight.await.is_ok());
}

#[tokio::test]
async fn a_sibling_of_a_volume_ejecting_alone_waits_and_decides_again_rather_than_taking_its_answer() {
    // A and B ejected together, one volume each: reading each other as still mounted, both
    // would unmount alone and leave an empty disk powered on. So B waits, then sees it's last.
    let (a, b) = ("volumes-cmdr-test-alone-a", "volumes-cmdr-test-alone-b");
    let key = DiskKey(7001);
    let (a_owns, a_owned_it) = tokio::sync::oneshot::channel::<()>();
    let (release, held) = tokio::sync::oneshot::channel::<()>();

    let a_flight = as_a_flight(a, move || async move {
        let ownership = owned(own_disk_for_one_volume(key, a));
        assert!(ownership.adopted().is_empty(), "a sibling's row stays usable");
        assert!(is_ejecting_alone(a), "the approver reads this ask as about A alone");
        let _ = a_owns.send(());
        let _ = held.await;
        drop(ownership);
        Ok(EjectOutcome::VolumeEjectedDiskStays)
    });
    let _ = a_owned_it.await;
    assert!(!ejecting_volume_ids().contains(&b.to_string()), "B wasn't adopted");
    assert!(!is_ejecting_alone(b));

    let b_flight = as_a_flight(b, move || async move {
        let DiskFlight::DecideAgainAfter(running) = own_disk_for_one_volume(key, b) else {
            panic!("A holds the disk for one volume, so B waits");
        };
        assert!(matches!(running.await, Ok(EjectOutcome::VolumeEjectedDiskStays)));
        // A landed, so B may claim the disk: all of it, as its last volume.
        let ownership = owned(join_or_own_disk(key, b, &[b.to_string()]));
        assert!(!is_ejecting_alone(b), "a whole-disk flight isn't alone");
        drop(ownership);
        Ok(EjectOutcome::DiskEjected)
    });

    let _ = release.send(());
    let (a_result, b_result) = tokio::join!(a_flight, b_flight);
    assert!(
        matches!(a_result, Ok(EjectOutcome::VolumeEjectedDiskStays)),
        "got {a_result:?}"
    );
    assert!(matches!(b_result, Ok(EjectOutcome::DiskEjected)), "got {b_result:?}");
    assert!(!is_ejecting_alone(a), "the claim leaves with its flight");
}

#[tokio::test]
async fn eject_all_waits_for_a_volume_ejecting_alone_rather_than_taking_its_answer() {
    let (a, b) = ("volumes-cmdr-test-all-a", "volumes-cmdr-test-all-b");
    let key = DiskKey(7002);
    let (a_owns, a_owned_it) = tokio::sync::oneshot::channel::<()>();
    let (release, held) = tokio::sync::oneshot::channel::<()>();
    let a_flight = as_a_flight(a, move || async move {
        let ownership = owned(own_disk_for_one_volume(key, a));
        let _ = a_owns.send(());
        let _ = held.await;
        drop(ownership);
        Ok(EjectOutcome::VolumeEjectedDiskStays)
    });
    let _ = a_owned_it.await;
    let b_flight = as_a_flight(b, move || async move {
        assert!(
            matches!(
                join_or_own_disk(key, b, &[a.to_string(), b.to_string()]),
                DiskFlight::DecideAgainAfter(_)
            ),
            "one volume's answer isn't this request's"
        );
        Ok(EjectOutcome::DiskEjected)
    });
    let b_result = b_flight.await;
    assert!(b_result.is_ok(), "got {b_result:?}");
    let _ = release.send(());
    let a_result = a_flight.await;
    assert!(a_result.is_ok(), "got {a_result:?}");
}

#[tokio::test]
async fn a_teardown_that_panics_answers_unexpected_and_clears_the_set() {
    let vid = "volumes-cmdr-test-panicking-flight";
    let result = join_or_start(vid, || async { explode() }).await;
    assert!(matches!(result, Err(EjectError::Unexpected { .. })), "got {result:?}");
    assert!(
        !ejecting_volume_ids().contains(&vid.to_string()),
        "a panic can't strand the volume in the ejecting set"
    );
}
