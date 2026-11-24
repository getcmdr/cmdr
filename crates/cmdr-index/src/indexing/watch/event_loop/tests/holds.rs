//! What the live loop holds while it runs: its share of the volume's hold.

use std::time::Duration;

use super::*;
use crate::indexing::hold::{self, HoldKind, Release, VolumeWork};
use crate::indexing::reconcile::reconciler::EventReconciler;
use crate::indexing::watch::branches::{self, WatchScope};
use crate::indexing::writer::IndexWriter;

/// The live loop reads the drive for every event it applies, and `shutdown` waits
/// only five seconds for it before it detaches. Past that it still holds the drive,
/// and it lets go only once it ends.
#[tokio::test(flavor = "multi_thread")]
async fn the_live_loop_holds_its_volume_until_it_ends() {
    let volume_id = "live-loop-test-holds";
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("live-loop.db");
    IndexStore::open(&db_path).expect("open store");
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).expect("spawn writer");
    let (watcher, watched) = tokio::sync::mpsc::unbounded_channel();
    let volume = VolumeWork::for_test(volume_id);
    let reconciler = EventReconciler::new_for(
        volume_id.to_string(),
        IndexPathSpace::root(),
        volume.child(HoldKind::LiveLoop),
    );

    let live = crate::indexing::host::runtime::spawn(run_live_event_loop(
        watched,
        reconciler,
        writer.clone(),
        crate::NoopEventSink::shared(),
        LiveConfig {
            volume_id: volume_id.to_string(),
            space: IndexPathSpace::root(),
            watcher_overflow: None,
            watch_ended: None,
            scope: WatchScope::WholeVolume(branches::live_for(volume_id)),
            history_check: None,
        },
    ));
    drop(volume);
    assert_eq!(
        hold::wait_until_released(volume_id, Duration::ZERO),
        Release::StillHeld(vec![(HoldKind::LiveLoop, 1)]),
        "the loop still holds the drive once its manager is gone"
    );

    drop(watcher);
    assert_eq!(
        live.await.expect("the loop ends once its watcher's channel closes"),
        LiveLoopEnd::Stopped,
        "a closed channel leaves the owner nothing to run"
    );
    assert_eq!(
        hold::wait_until_released(volume_id, Duration::from_secs(5)),
        Release::Released,
        "and lets go as it ends"
    );
    writer.shutdown();
}
