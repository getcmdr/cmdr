//! A cross-volume move off a LOCAL source holding Locked items (#408): asked
//! once before any byte moves, then the sweep unlocks each original right before
//! deleting it (Move anyway), or the walk leaves it out whole (Skip). The local
//! engines' cells: `../metadata_matrix_tests.rs`.

use super::test_support::make_state;
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::{LockedKept, LockedSourcesAnswer};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::metadata_fixture::{LOCKED_FLAG, MetadataFixture, flags};
use cmdr_fs::volume::InMemoryVolume;
use cmdr_local::LocalVolume;

fn local_source(fixture: &MetadataFixture) -> Arc<dyn Volume> {
    Arc::new(LocalVolume::local_folder(
        "Source",
        fixture.join(""),
        crate::host::testing::pane_order_host(),
    ))
}

fn dest() -> Arc<dyn Volume> {
    Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000))
}

fn answering(state: &Arc<WriteOperationState>, answer: LockedSourcesAnswer) -> Arc<CollectorEventSink> {
    let events = Arc::new(CollectorEventSink::new());
    let state = Arc::clone(state);
    events.answer_locked_with(move |_| {
        state.locked_sources.answer(answer);
    });
    events
}

async fn run(
    state: &Arc<WriteOperationState>,
    events: &Arc<CollectorEventSink>,
    source: Arc<dyn Volume>,
    sources: &[PathBuf],
    dest: &Arc<dyn Volume>,
) {
    move_volumes_with_progress(
        Arc::clone(events) as Arc<dyn OperationEventSink>,
        "op-move-locked",
        state,
        source,
        sources,
        Arc::clone(dest),
        Path::new("/"),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("the move completes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn move_anyway_deletes_each_locked_original_once_its_copy_landed() {
    let engine = TestOperations::new();
    let fixture = MetadataFixture::new("volume_move_locked");
    let folder = fixture.join("album");
    std::fs::create_dir(&folder).expect("create the folder");
    fixture.locked_file("album/locked.txt");
    fixture.deny_delete_file("album/acl.txt");
    let top = fixture.locked_file("top.txt");
    let dest = dest();
    let state = make_state(&engine);
    let events = answering(&state, LockedSourcesAnswer::MoveAnyway);

    run(
        &state,
        &events,
        local_source(&fixture),
        &[folder.clone(), top.clone()],
        &dest,
    )
    .await;

    for landed in ["/album/locked.txt", "/album/acl.txt", "/top.txt"] {
        assert!(dest.exists(Path::new(landed)).await, "{landed} landed");
    }
    assert!(!folder.exists() && !top.exists(), "every original is gone");
    let asked = events.locked_sources.lock_ignore_poison().clone();
    assert_eq!(asked.len(), 1, "asked once for the whole move");
    assert_eq!(asked[0].locked_count, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skip_leaves_each_locked_item_out_and_moves_the_rest() {
    let engine = TestOperations::new();
    let fixture = MetadataFixture::new("volume_move_locked_skip");
    let folder = fixture.join("album");
    std::fs::create_dir(&folder).expect("create the folder");
    let inner = fixture.locked_file("album/locked.txt");
    std::fs::write(folder.join("plain.txt"), b"plain").expect("write");
    let top = fixture.locked_file("top.txt");
    let dest = dest();
    let state = make_state(&engine);
    let events = answering(&state, LockedSourcesAnswer::Skip);

    run(
        &state,
        &events,
        local_source(&fixture),
        &[folder.clone(), top.clone()],
        &dest,
    )
    .await;

    assert!(dest.exists(Path::new("/album/plain.txt")).await, "the rest moved");
    assert!(!folder.join("plain.txt").exists());
    assert!(
        !dest.exists(Path::new("/album/locked.txt")).await,
        "the Locked file stayed out"
    );
    assert!(!dest.exists(Path::new("/top.txt")).await);
    assert_eq!(flags(&inner) & LOCKED_FLAG, LOCKED_FLAG, "it stays, still Locked");
    assert_eq!(flags(&top) & LOCKED_FLAG, LOCKED_FLAG);
    let complete = events.complete.lock_ignore_poison().clone();
    assert_eq!(complete.len(), 1);
    assert_eq!(complete[0].files_skipped, 2);
    assert_eq!(
        complete[0].locked_kept,
        Some(LockedKept {
            skipped: 2,
            system_locked: 0
        })
    );
}

/// A source that isn't local can't wear Locked: nothing is walked, nobody asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_that_isnt_local_asks_nothing() {
    let engine = TestOperations::new();
    let source: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    source.create_file(Path::new("/a.txt"), b"alpha").await.unwrap();
    let dest = dest();
    let state = make_state(&engine);
    let events = Arc::new(CollectorEventSink::new());

    run(&state, &events, source, &[PathBuf::from("/a.txt")], &dest).await;

    assert!(events.locked_sources.lock_ignore_poison().is_empty());
    assert!(dest.exists(Path::new("/a.txt")).await);
}
