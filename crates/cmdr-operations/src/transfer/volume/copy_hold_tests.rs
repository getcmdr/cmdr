//! A copy holds its source volume's [`TransferHold`] for its whole run: taken
//! when it starts running, kept while it's paused between files, let go of
//! when it settles, however it settles. A restic repo's index rides on it
//! (#629): without it, a paused copy out of a backup lost its index to the idle
//! drop or to another repo, and the next file paid a reload.

use crate::event_sinks::{CollectorEventSink, OperationEventSink};
use crate::manager::{DeferredStart, ManagedTaskGuard, OperationDescriptor};
use crate::testing::{TestOperations, pause_write_operation, resume_write_operation};
use crate::types::{OperationSummaryText, RollbackSupport, VolumeCopyConfig, WriteOperationType};
use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::wait_until_async;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_operation_log::types::{Initiator, OperationId};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

const WITHIN: Duration = Duration::from_secs(10);

/// A source whose holds count, two files on it, and an empty destination.
async fn volumes(name: &str) -> (Arc<InMemoryVolume>, Arc<dyn Volume>) {
    let source = Arc::new(
        InMemoryVolume::new("Backup")
            .with_lane_key(format!("hold-src-{name}"))
            .counting_transfer_holds(),
    );
    for file in ["/a.txt", "/b.txt"] {
        source
            .create_file(AppPath::from_normalized(file).expect("a canonical path"), b"restored")
            .await
            .expect("seed");
    }
    let dest: Arc<dyn Volume> = Arc::new(
        InMemoryVolume::new("Disk")
            .with_lane_key(format!("hold-dst-{name}"))
            .with_space_info(1_000_000, 900_000),
    );
    (source, dest)
}

/// Holds `source`'s lane with a placeholder operation until the returned sender
/// fires, so a copy started meanwhile waits Queued.
async fn hold_the_lane(engine: &TestOperations, source: &InMemoryVolume) -> oneshot::Sender<()> {
    let id = OperationId::from(format!("lane-holder-{}", source.lane_key().as_str()));
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel::<()>();
    let descriptor = OperationDescriptor {
        operation_id: id.clone(),
        operation_type: WriteOperationType::Copy,
        lanes: vec![source.lane_key()],
        volume_ids: vec![],
        summary: OperationSummaryText::default(),
        rollback_support: RollbackSupport::Supported,
        preview_id: None,
        reverses: None,
    };
    let deferred: DeferredStart = Box::new(move |ops: crate::Operations| {
        Box::pin(async move {
            let guard = ManagedTaskGuard::new(ops.manager(), id.clone());
            let _ = started_tx.send(());
            let _ = release_rx.await;
            guard.disarm();
            ops.manager().on_settled(&id);
        })
    });
    engine.manager().spawn_managed(descriptor, engine.new_state(), deferred);
    started_rx.await.expect("the placeholder holds the lane");
    release_tx
}

/// Starts a copy of both files, paused before its first one. It waits Queued
/// behind [`hold_the_lane`] until the pause is in: started on free lanes, it
/// could finish both files before the pause lands (a raced test, #732).
async fn paused_copy(
    engine: &TestOperations,
    source: &Arc<InMemoryVolume>,
    dest: &Arc<dyn Volume>,
    events: &Arc<CollectorEventSink>,
) -> OperationId {
    let lane = hold_the_lane(engine, source).await;
    let sink: Arc<dyn OperationEventSink> = Arc::clone(events) as Arc<dyn OperationEventSink>;
    let started = engine
        .copy_between_volumes(
            sink,
            "backup".to_string(),
            Arc::clone(source) as Arc<dyn Volume>,
            vec![PathBuf::from("/a.txt"), PathBuf::from("/b.txt")],
            "disk".to_string(),
            Arc::clone(dest),
            PathBuf::from("/"),
            VolumeCopyConfig::default(),
            Initiator::User,
            None,
        )
        .await
        .expect("the copy starts");
    let id = OperationId::from(started.operation_id);
    assert!(pause_write_operation(engine, &id), "the copy is live");
    lane.send(()).expect("the placeholder still holds the lane");
    id
}

async fn settled(events: &CollectorEventSink) {
    wait_until_async(WITHIN, "the copy settles", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
}

/// ❗ Held while the copy is paused (nothing open on the source then), and let
/// go of once it completes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paused_copy_holds_its_source_and_lets_go_when_it_completes() {
    let engine = TestOperations::new();
    let (source, dest) = volumes("complete").await;
    let events = Arc::new(CollectorEventSink::new());

    let id = paused_copy(&engine, &source, &dest, &events).await;

    wait_until_async(WITHIN, "the running copy holds its source", || {
        source.live_transfer_holds() == 1
    })
    .await;
    assert!(!dest.exists(app_path!("/b.txt")).await, "still paused");

    assert!(resume_write_operation(&engine, &id));
    settled(&events).await;
    assert!(dest.exists(app_path!("/b.txt")).await, "the copy finished");
    assert_eq!(source.live_transfer_holds(), 0, "let go of once settled");
}

/// A copy cancelled while paused lets go of its hold too: ❌ never one left
/// pinned by a transfer that ended any other way.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copy_cancelled_while_paused_lets_go_of_its_source() {
    let engine = TestOperations::new();
    let (source, dest) = volumes("cancel").await;
    let events = Arc::new(CollectorEventSink::new());

    let id = paused_copy(&engine, &source, &dest, &events).await;
    wait_until_async(WITHIN, "the running copy holds its source", || {
        source.live_transfer_holds() == 1
    })
    .await;

    engine.cancel_write_operation(&id, false);
    settled(&events).await;
    assert_eq!(source.live_transfer_holds(), 0);
}
