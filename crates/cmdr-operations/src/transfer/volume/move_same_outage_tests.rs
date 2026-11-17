//! A same-volume move whose share drops while its preflight lists a source's
//! parent waits for the share, the way every other transfer step does
//! (`../volume_outage.rs`), then lists again and renames (cmdr-reports#112).
//!
//! The listing runs before any rename, so it's safe to run twice: these cells
//! pin that it does, and that a share that never comes back reads as the lost
//! connection it is, not a device error.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::super::move_same::move_within_same_volume_with_progress;
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::outage_slot::OutagePolicy;
use crate::testing::TestOperations;
use crate::transfer::outage_volume_test_support::OutageVolume;
use crate::types::WriteOperationError;
use cmdr_fs::testing::wait_until_async;
use cmdr_fs::volume::{InMemoryVolume, Volume};

const WAIT: Duration = Duration::from_secs(10);
const CADENCE: &[Duration] = &[Duration::from_millis(10), Duration::from_millis(20)];

fn outage_state(engine: &TestOperations, budget: Duration) -> Arc<WriteOperationState> {
    Arc::new(
        engine
            .new_operation_state(Duration::from_millis(50))
            .with_outage_policy(OutagePolicy {
                budget,
                cadence: CADENCE,
                poll_every: Duration::from_millis(5),
                attempt_timeout: Duration::from_secs(1),
            }),
    )
}

/// A share holding `/batch/f0.txt` and an empty `/dest`.
async fn share(lane: &str) -> (Arc<InMemoryVolume>, Arc<OutageVolume>) {
    let inner = Arc::new(InMemoryVolume::new("Testnas").with_space_info(10_000_000, 10_000_000));
    inner.create_directory(Path::new("/batch")).await.unwrap();
    inner.create_directory(Path::new("/dest")).await.unwrap();
    inner.create_file(Path::new("/batch/f0.txt"), b"file 0").await.unwrap();
    let share = OutageVolume::wrap(Arc::clone(&inner), lane);
    (inner, share)
}

async fn move_f0(
    engine: &TestOperations,
    op: &str,
    state: &Arc<WriteOperationState>,
    share: &Arc<OutageVolume>,
) -> Result<(), WriteOperationError> {
    let _op = engine.register_state(op, Arc::clone(state));
    move_within_same_volume_with_progress(
        Arc::new(CollectorEventSink::new()),
        op,
        state,
        Arc::clone(share) as Arc<dyn Volume>,
        &[PathBuf::from("/batch/f0.txt")],
        Path::new("/dest"),
        &VolumeCopyConfig::default(),
    )
    .await
}

/// The share drops as the preflight lists `/batch`. The move waits, lists it
/// again once the share is back, and renames the file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_parent_listing_the_share_drops_under_waits_and_the_move_completes() {
    let engine = TestOperations::new();
    let (inner, share) = share("outage-same-move-listing").await;
    share.drop_at_listing_of(Path::new("/batch"));
    let state = outage_state(&engine, Duration::from_secs(30));
    {
        let state = Arc::clone(&state);
        let share = Arc::clone(&share);
        tokio::spawn(async move {
            wait_until_async(WAIT, "the move to wait for the share", || {
                state.outage.waiting_for().is_some()
            })
            .await;
            share.become_reachable();
        });
    }

    move_f0(&engine, "outage-same-move-listing", &state, &share)
        .await
        .expect("a share that comes back within the budget must not fail the move");

    assert!(
        share.reconnect_attempts() >= 1,
        "the share really dropped under the listing"
    );
    assert_eq!(
        share.listings_of(Path::new("/batch")),
        2,
        "the dropped listing ran again"
    );
    assert!(!inner.exists(Path::new("/batch/f0.txt")).await, "the source moved away");
    assert!(inner.exists(Path::new("/dest/f0.txt")).await, "and arrived");
}

/// The share never comes back: after the budget, the move stops with "Lost the
/// connection to Testnas", the folder it was listing as context, and nothing
/// renamed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_parent_listing_whose_share_stays_gone_says_it_lost_the_connection() {
    let engine = TestOperations::new();
    let (inner, share) = share("outage-same-move-gone").await;
    share.drop_at_listing_of(Path::new("/batch"));
    let state = outage_state(&engine, Duration::from_millis(300));

    let error = move_f0(&engine, "outage-same-move-gone", &state, &share)
        .await
        .expect_err("a share that never comes back still fails the move");

    assert!(
        matches!(
            &error,
            WriteOperationError::ServerConnectionLost { path, volume_name, gave_up_after_secs: Some(_) }
                if path == "/batch" && volume_name == "Testnas"
        ),
        "the connection is the cause, named with the wait: {error:?}"
    );
    assert!(share.reconnect_attempts() > 1, "after really trying");
    assert!(inner.exists(Path::new("/batch/f0.txt")).await, "nothing moved");
}
