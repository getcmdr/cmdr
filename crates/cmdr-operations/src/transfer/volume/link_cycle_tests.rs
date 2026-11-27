//! A copy that follows links (onto a destination that can't hold one) never
//! follows one back into a folder it is already inside (ERR-BGEKC).
//!
//! A link to its own folder or one above it makes the tree endless: each pass
//! through it copies the same folder one level deeper, so the copy wrote the
//! selection three times over before the person cancelled, and its counts ran to
//! three times the scan's total. `merge.rs::descends_into_a_cycle` decides;
//! `DETAILS.md` § "A followed link never leads back up its own path".
#![cfg(unix)] // POSIX links on a `LocalPosixVolume`.

use cmdr_fs::app_path;
use cmdr_fs::testing::{TestDir, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::copy::copy_volumes_with_progress;
use super::transfer_error::WriteFailure;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::{VolumeCopyConfig, WriteOperationPhase};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_local::LocalPosixVolume;

/// A remote with no links at all (an SMB share, to the engine).
fn no_links() -> Arc<InMemoryVolume> {
    Arc::new(InMemoryVolume::new("No links").with_space_info(100_000_000, 100_000_000))
}

fn local_folder(dir: &Path) -> Arc<dyn Volume> {
    Arc::new(LocalPosixVolume::local_folder(
        "Local",
        dir.to_path_buf(),
        crate::host::testing::pane_order_host(),
    ))
}

/// Copies `/tree` onto `dest`, bounded so an endless walk fails the test instead
/// of hanging it.
async fn copy_tree(dir: &Path, dest: Arc<InMemoryVolume>) -> (Result<(), WriteFailure>, Arc<CollectorEventSink>) {
    let engine = TestOperations::new();
    let events = Arc::new(CollectorEventSink::new());
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let sources = crate::testing::app_paths_of(&[PathBuf::from("/tree")]);
    let dest_root = crate::testing::app_path_of(Path::new("/"));
    let config = VolumeCopyConfig {
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };
    let run = copy_volumes_with_progress(
        events.clone(),
        "op-link-cycle",
        &state,
        local_folder(dir),
        &sources,
        dest,
        &dest_root,
        &config,
    );
    let result = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the copy ends: a link loop never walks forever");
    (result, events)
}

/// Every copying event's done count stays within its total. (A scanning event's
/// total is 0 until the scan has counted, by design.)
fn never_past_the_total(events: &CollectorEventSink) {
    let progress = events.progress.lock_ignore_poison();
    let copying: Vec<_> = progress
        .iter()
        .filter(|event| event.phase == WriteOperationPhase::Copying)
        .collect();
    assert!(!copying.is_empty(), "the copy reported progress");
    for event in copying {
        assert!(
            event.files_done <= event.files_total,
            "files {} of {}",
            event.files_done,
            event.files_total
        );
        assert!(
            event.bytes_done <= event.bytes_total,
            "bytes {} of {}",
            event.bytes_done,
            event.bytes_total
        );
    }
}

/// The reporter's shape: a folder holding a link to the folder above it, and
/// one to itself. Each lands once, the links are counted skips, and the bars
/// end where the scan said they would.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_back_up_its_own_path_is_a_counted_skip_not_another_copy() {
    let dir = TestDir::new("link-cycle-up");
    std::fs::create_dir_all(dir.join("tree/sub")).unwrap();
    std::fs::write(dir.join("tree/keep.txt"), b"kept").unwrap();
    std::fs::write(dir.join("tree/sub/a.txt"), b"aaa").unwrap();
    symlink("..", dir.join("tree/sub/up")).unwrap();
    symlink(".", dir.join("tree/sub/here")).unwrap();
    let dest = no_links();

    let (result, events) = copy_tree(&dir, dest.clone()).await;

    assert!(result.is_ok(), "{result:?}");
    assert!(dest.exists(app_path!("/tree/keep.txt")).await);
    assert!(dest.exists(app_path!("/tree/sub/a.txt")).await);
    assert!(!dest.exists(app_path!("/tree/sub/up")).await, "the loop isn't walked");
    assert!(!dest.exists(app_path!("/tree/sub/here")).await, "the loop isn't walked");
    let complete = events.complete.lock_ignore_poison();
    assert_eq!(complete[0].files_skipped, 2);
    drop(complete);
    never_past_the_total(&events);
}

/// Two links that lead to each other's folders: neither is an ancestor of
/// itself by name, but walking either reaches the other's folder again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_links_leading_to_each_other_stop_at_the_second() {
    let dir = TestDir::new("link-cycle-pair");
    std::fs::create_dir_all(dir.join("tree/a")).unwrap();
    std::fs::create_dir_all(dir.join("tree/b")).unwrap();
    std::fs::write(dir.join("tree/a/a.txt"), b"aaa").unwrap();
    std::fs::write(dir.join("tree/b/b.txt"), b"bbb").unwrap();
    symlink("../b", dir.join("tree/a/to_b")).unwrap();
    symlink("../a", dir.join("tree/b/to_a")).unwrap();
    let dest = no_links();

    let (result, _events) = copy_tree(&dir, dest.clone()).await;

    assert!(result.is_ok(), "{result:?}");
    // Each link to a sibling is followed once, as a NAS copy should hold the files.
    assert!(dest.exists(app_path!("/tree/a/to_b/b.txt")).await);
    assert!(dest.exists(app_path!("/tree/b/to_a/a.txt")).await);
    // And the way back is where it stops.
    assert!(!dest.exists(app_path!("/tree/a/to_b/to_a")).await);
    assert!(!dest.exists(app_path!("/tree/b/to_a/to_b")).await);
}

/// A move walks the same way, and its sweep leaves the skipped link where it is
/// (so its folder stays too), rather than delete a source it never moved.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_skips_the_loop_and_leaves_the_link_in_the_source() {
    let dir = TestDir::new("link-cycle-move");
    std::fs::create_dir_all(dir.join("tree/sub")).unwrap();
    std::fs::write(dir.join("tree/sub/a.txt"), b"aaa").unwrap();
    symlink("..", dir.join("tree/sub/up")).unwrap();
    let dest = no_links();
    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let sources = crate::testing::app_paths_of(&[PathBuf::from("/tree")]);
    let dest_root = crate::testing::app_path_of(Path::new("/"));
    let config = VolumeCopyConfig::default();
    let run = super::move_cross::move_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-link-cycle-move",
        &state,
        local_folder(&dir),
        &sources,
        Arc::clone(&dest) as Arc<dyn Volume>,
        &dest_root,
        &config,
    );
    let result = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the move ends: a link loop never walks forever");

    assert!(result.is_ok(), "{result:?}");
    assert!(dest.exists(app_path!("/tree/sub/a.txt")).await);
    assert!(!dest.exists(app_path!("/tree/sub/up")).await);
    assert!(!dir.join("tree/sub/a.txt").exists(), "what moved left the source");
    assert!(
        std::fs::symlink_metadata(dir.join("tree/sub/up")).is_ok(),
        "the skipped link stays"
    );
}

/// A link to a folder outside the selection that holds no way back is still
/// followed whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_to_a_folder_elsewhere_is_still_followed() {
    let dir = TestDir::new("link-cycle-none");
    std::fs::create_dir_all(dir.join("media/clips")).unwrap();
    std::fs::write(dir.join("media/clips/c.mov"), b"clip").unwrap();
    std::fs::create_dir_all(dir.join("tree")).unwrap();
    symlink("../media", dir.join("tree/Original Media")).unwrap();
    let dest = no_links();

    let (result, events) = copy_tree(&dir, dest.clone()).await;

    assert!(result.is_ok(), "{result:?}");
    assert!(dest.exists(app_path!("/tree/Original Media/clips/c.mov")).await);
    assert_eq!(events.complete.lock_ignore_poison()[0].files_skipped, 0);
}
