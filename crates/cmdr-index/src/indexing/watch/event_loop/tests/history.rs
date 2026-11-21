//! The purged-history check a resumed live loop runs on its first event
//! (`LiveConfig::history_check`), and the pure decision behind it.

use super::*;
use crate::indexing::hold::{HoldKind, VolumeWork};
use crate::indexing::paths::app_text::native_text;
use crate::indexing::reconcile::reconciler::EventReconciler;
use crate::indexing::watch::branches::{self, WatchScope};
use crate::indexing::writer::IndexWriter;

const SINCE: u64 = 4_000_000_000;

#[test]
fn a_first_event_right_after_the_stored_id_is_intact_history() {
    assert!(!history_was_purged(SINCE, SINCE + 1));
    assert!(
        !history_was_purged(SINCE, SINCE + JOURNAL_GAP_THRESHOLD),
        "the threshold itself is still intact"
    );
    assert!(history_was_purged(SINCE, SINCE + JOURNAL_GAP_THRESHOLD + 1));
    assert!(
        !history_was_purged(u64::MAX, u64::MAX),
        "no overflow at the top of the range"
    );
}

/// Runs a live loop with `history_check`, feeds it one event, and answers the
/// epoch the index ends at, against the one it started at.
async fn epochs_after_first_event(name: &str, history_check: Option<u64>, first_event_id: u64) -> (u64, u64) {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("history.db");
    IndexStore::open(&db_path).expect("open store");
    let before = IndexStore::read_current_epoch(&IndexStore::open_read_connection(&db_path).expect("read"))
        .expect("epoch before");
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).expect("spawn writer");
    let (watcher, watched) = tokio::sync::mpsc::unbounded_channel();
    let volume = VolumeWork::for_test(name);
    let reconciler = EventReconciler::new_for(
        name.to_string(),
        IndexPathSpace::root(),
        volume.child(HoldKind::LiveLoop),
    );
    let live = crate::indexing::host::runtime::spawn(run_live_event_loop(
        watched,
        reconciler,
        writer.clone(),
        crate::NoopEventSink::shared(),
        LiveConfig {
            volume_id: name.to_string(),
            space: IndexPathSpace::root(),
            watcher_overflow: None,
            scope: WatchScope::WholeVolume(branches::live_for(name)),
            history_check,
        },
    ));
    let gone = dir.path().join("never-existed.txt");
    watcher
        .send(make_event(
            &native_text(&gone),
            first_event_id,
            watcher::FsEventFlags {
                item_modified: true,
                item_is_file: true,
                ..Default::default()
            },
        ))
        .expect("send");
    drop(watcher);
    let _ = live.await.expect("the loop ends once its channel closes");
    writer.flush().await.expect("flush");
    let after = IndexStore::read_current_epoch(&IndexStore::open_read_connection(&db_path).expect("read"))
        .expect("epoch after");
    writer.shutdown();
    drop(volume);
    (before, after)
}

/// The branch-watch resume that #751 made replay across a wide id gap: if the
/// journal dropped what came after the stored id, the covered rows can't claim to
/// be current, so the loop says they're stale before it writes anything.
#[tokio::test(flavor = "multi_thread")]
async fn purged_history_marks_the_covered_rows_stale() {
    let (before, after) =
        epochs_after_first_event("history-purged", Some(SINCE), SINCE + JOURNAL_GAP_THRESHOLD + 1).await;
    assert!(
        after > before,
        "the epoch moves, so the read side renders the rows stale"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn intact_history_keeps_the_rows_current() {
    let (before, after) = epochs_after_first_event("history-intact", Some(SINCE), SINCE + 1).await;
    assert_eq!(after, before);
}

/// A loop asked to check nothing judges nothing, however far its first event lands:
/// a scan's own loop starts right behind the walk.
#[tokio::test(flavor = "multi_thread")]
async fn a_loop_with_no_history_check_never_bumps() {
    let (before, after) = epochs_after_first_event("history-unchecked", None, SINCE + JOURNAL_GAP_THRESHOLD * 5).await;
    assert_eq!(after, before);
}
