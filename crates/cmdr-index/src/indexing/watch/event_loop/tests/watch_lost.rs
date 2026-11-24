//! How the live loop ends when its watcher's channel closes: a stop, or a watch the
//! OS ended on its own (#1363).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::indexing::hold::{HoldKind, VolumeWork};
use crate::indexing::reconcile::reconciler::EventReconciler;
use crate::indexing::watch::branches::{self, WatchScope};
use crate::indexing::writer::IndexWriter;

/// Run a live loop until its channel closes, with the watcher's ended flag `ended`.
async fn end_of_a_loop(volume_id: &str, ended: bool) -> LiveLoopEnd {
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
    let flag = Arc::new(AtomicBool::new(false));

    let live = crate::indexing::host::runtime::spawn(run_live_event_loop(
        watched,
        reconciler,
        writer.clone(),
        crate::NoopEventSink::shared(),
        LiveConfig {
            volume_id: volume_id.to_string(),
            space: IndexPathSpace::root(),
            watcher_overflow: None,
            watch_ended: Some(Arc::clone(&flag)),
            scope: WatchScope::WholeVolume(branches::live_for(volume_id)),
            history_check: None,
        },
    ));
    // The order a backend keeps: the flag goes up, THEN its sender drops.
    flag.store(ended, Ordering::Release);
    drop(watcher);
    let end = live.await.expect("the loop ends once its watcher's channel closes");
    drop(volume);
    writer.shutdown();
    end
}

/// #1363: a watch that died closed the channel the way a stop does, so the loop ended
/// owing nothing and the drive's index read live for the rest of the session.
#[tokio::test(flavor = "multi_thread")]
async fn a_watch_the_os_ended_asks_its_owner_for_a_re_arm() {
    assert!(matches!(
        end_of_a_loop("live-loop-test-watch-lost", true).await,
        LiveLoopEnd::WatchLost { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_that_was_stopped_leaves_nothing_owed() {
    assert_eq!(
        end_of_a_loop("live-loop-test-watch-stopped", false).await,
        LiveLoopEnd::Stopped
    );
}
