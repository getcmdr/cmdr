//! A folder tree of any depth copies and moves across volumes on the stack a
//! copy really runs on (#1541).
//!
//! The merge walk and a move's source sweep keep their depth on the heap, ❌
//! never in nested futures: each once nested one future per folder level, and a
//! 2 MiB tokio worker (Tauri's default, which runs copies) overflowed in the walk
//! between 48 and 63 levels in a release build, and in the sweep between 300 and
//! 350 levels in a debug one. These cells run the whole pipeline on exactly such
//! a worker, so anything that starts nesting per level again aborts the test
//! binary here rather than the app. `DETAILS.md` § "The walk keeps its depth on
//! the heap".
//!
//! The source is a local folder: its listing reads one directory, where
//! `InMemoryVolume`'s scans its whole store per call, which made a deep tree
//! cubic and spent the test's budget on the double rather than the walk.
#![cfg(unix)] // A 400-level path outgrows a Windows path without the long-path prefix.

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_local::LocalPosixVolume;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::copy::copy_volumes_with_progress;
use super::move_cross::move_volumes_with_progress;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::VolumeCopyConfig;

/// Past both old ceilings on this stack (the sweep's was the higher, 300–350 in a
/// debug build), and inside macOS's 1,024-byte `PATH_MAX` with the temp folder in
/// front: each level adds two bytes (`d/`), about 920 in all.
const DEPTH: usize = 400;

/// The stack a Tauri copy runs on: tokio's default worker.
const WORKER_STACK: usize = 2 * 1024 * 1024;

/// The tree, relative to the source folder: `deep`, then `DEPTH` nested `d`
/// folders, with one file at the bottom.
fn bottom_folder() -> String {
    format!("deep{}", "/d".repeat(DEPTH))
}

/// `deep` and its levels under a fresh folder.
fn deep_folder(label: &str) -> TestDir {
    let dir = TestDir::new(label);
    let bottom = dir.join(bottom_folder());
    std::fs::create_dir_all(&bottom).unwrap();
    std::fs::write(bottom.join("bottom.txt"), b"bottom").unwrap();
    dir
}

/// The folder as a local volume, which scans with its own walk.
fn local(dir: &TestDir) -> Arc<dyn Volume> {
    Arc::new(LocalPosixVolume::local_folder(
        "Local",
        dir.to_path_buf(),
        crate::host::testing::pane_order_host(),
    ))
}

/// The folder behind a backend that scans through the shared `scan_walk`, as
/// ADB, NFS, S3, SFTP, and WebDAV sources do, before the copy walks it.
fn scanned_by_the_shared_walk(dir: &TestDir) -> Arc<dyn Volume> {
    super::link_following_backend_tests::scan_walking_volume(dir)
}

fn app(path: &str) -> AppPathBuf {
    AppPath::from_normalized(path).expect("a canonical path").to_owned()
}

/// A remote with no links (an SMB share, to the engine), so the copy streams
/// through the merge walk.
fn no_links_dest() -> Arc<InMemoryVolume> {
    Arc::new(InMemoryVolume::new("Dest").with_space_info(100_000_000, 100_000_000))
}

/// Runs `body` on a multi-thread runtime whose workers have [`WORKER_STACK`],
/// spawned so it polls on a worker rather than this test's own thread.
fn on_a_copy_worker<F>(body: impl FnOnce() -> F) -> F::Output
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(WORKER_STACK)
        .enable_all()
        .build()
        .unwrap();
    let spawned = runtime.spawn(body());
    runtime
        .block_on(spawned)
        .expect("the transfer's task ends without a panic")
}

/// Every level arrived, and the file at the bottom of them.
fn holds_the_whole_tree(dest: &InMemoryVolume) {
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        let bottom = format!("/{}", bottom_folder());
        assert!(dest.exists(&app(&bottom)).await, "every level landed");
        assert!(
            dest.exists(&app(&format!("{bottom}/bottom.txt"))).await,
            "the bottom file landed"
        );
    });
}

/// Copies `/deep` off `source` on a copy worker and checks every level landed.
fn copies_whole(source: Arc<dyn Volume>) {
    let dest = no_links_dest();
    let events = Arc::new(CollectorEventSink::new());
    let run = {
        let (dest, events) = (Arc::clone(&dest), Arc::clone(&events));
        || async move {
            let state = Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(50)));
            copy_volumes_with_progress(
                events,
                "op-deep-copy",
                &state,
                source,
                &crate::testing::app_paths_of(&[PathBuf::from("/deep")]),
                dest as Arc<dyn Volume>,
                &app("/"),
                &VolumeCopyConfig::default(),
            )
            .await
        }
    };

    let result = on_a_copy_worker(run);

    assert!(result.is_ok(), "{result:?}");
    holds_the_whole_tree(&dest);
    assert_eq!(events.complete.lock_ignore_poison()[0].files_processed, 1);
}

/// Moves `/deep` off `source` (over `dir`) on a copy worker: the walk, then the
/// sweep taking every level out of the source, deepest first.
fn moves_whole(source: Arc<dyn Volume>, dir: &TestDir) {
    let dest = no_links_dest();
    let run = {
        let dest = Arc::clone(&dest);
        || async move {
            let state = Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(50)));
            move_volumes_with_progress(
                Arc::new(CollectorEventSink::new()),
                "op-deep-move",
                &state,
                source,
                &crate::testing::app_paths_of(&[PathBuf::from("/deep")]),
                dest as Arc<dyn Volume>,
                &app("/"),
                &VolumeCopyConfig::default(),
            )
            .await
        }
    };

    let result = on_a_copy_worker(run);

    assert!(result.is_ok(), "{result:?}");
    holds_the_whole_tree(&dest);
    assert!(
        !dir.join("deep").exists(),
        "the sweep took the moved tree out of the source"
    );
}

#[test]
fn a_four_hundred_level_tree_copies_on_a_copy_workers_stack() {
    let dir = deep_folder("deep-tree-copy");
    copies_whole(local(&dir));
}

#[test]
fn a_four_hundred_level_tree_moves_on_a_copy_workers_stack() {
    let dir = deep_folder("deep-tree-move");
    moves_whole(local(&dir), &dir);
}

/// The shared pre-copy scan walks the whole tree before the copy does.
#[test]
fn a_four_hundred_level_tree_copies_off_a_source_the_shared_scan_walks() {
    let dir = deep_folder("deep-tree-copy-shared-scan");
    copies_whole(scanned_by_the_shared_walk(&dir));
}

#[test]
fn a_four_hundred_level_tree_moves_off_a_source_the_shared_scan_walks() {
    let dir = deep_folder("deep-tree-move-shared-scan");
    moves_whole(scanned_by_the_shared_walk(&dir), &dir);
}
