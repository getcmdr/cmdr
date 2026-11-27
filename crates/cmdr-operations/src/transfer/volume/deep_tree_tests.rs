//! A folder tree of any depth copies and moves across volumes on the stack a
//! copy really runs on (#1541).
//!
//! The merge walk's depth costs heap, ❌ never stack: a walk that nested one
//! future per folder level overflowed a 2 MiB tokio worker (Tauri's default,
//! which runs copies) between 48 and 63 levels in a release build, and a debug
//! build's frames are several times bigger. These cells run the whole pipeline on
//! exactly such a worker, so a walk that starts nesting per level again aborts the
//! test binary here rather than the app. `DETAILS.md` § "The walk keeps its depth
//! on the heap".

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::copy::copy_volumes_with_progress;
use super::move_cross::move_volumes_with_progress;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::VolumeCopyConfig;

/// Far past the old ceiling, and still inside every `PATH_MAX` (each level adds
/// two bytes: `d/`).
const DEPTH: usize = 200;

/// The stack a Tauri copy runs on: tokio's default worker.
const WORKER_STACK: usize = 2 * 1024 * 1024;

/// `/deep`, then `DEPTH` nested `d` folders, each holding one file (`f`, its
/// depth as its bytes), with one more file at the bottom.
async fn deep_source() -> Arc<InMemoryVolume> {
    let source = Arc::new(InMemoryVolume::new("Source").with_space_info(100_000_000, 100_000_000));
    let mut level = String::from("/deep");
    source.create_directory(&app(&level)).await.unwrap();
    for depth in 0..DEPTH {
        level.push_str("/d");
        source.create_directory(&app(&level)).await.unwrap();
        source
            .create_file(&app(&format!("{level}/f")), depth.to_string().as_bytes())
            .await
            .unwrap();
    }
    source
        .create_file(&app(&format!("{level}/bottom.txt")), b"bottom")
        .await
        .unwrap();
    source
}

/// The path of the folder at `depth` (0 is `/deep/d`).
fn level_at(depth: usize) -> String {
    format!("/deep{}", "/d".repeat(depth + 1))
}

fn app(path: &str) -> AppPathBuf {
    AppPath::from_normalized(path).expect("a canonical path").to_owned()
}

/// Runs `body` on a multi-thread runtime whose workers have [`WORKER_STACK`],
/// spawned so it polls on a worker rather than this test's own thread.
fn on_a_copy_worker<F>(body: impl FnOnce() -> F + Send + 'static) -> F::Output
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

/// A remote with no links (an SMB share, to the engine), so nothing short-cuts
/// the streaming walk.
fn no_links_dest() -> Arc<InMemoryVolume> {
    Arc::new(InMemoryVolume::new("Dest").with_space_info(100_000_000, 100_000_000))
}

/// Every level and its file arrived, bottom included.
async fn holds_the_whole_tree(dest: &InMemoryVolume) {
    for depth in 0..DEPTH {
        let file = format!("{}/f", level_at(depth));
        assert!(dest.exists(&app(&file)).await, "{file} landed");
    }
    let bottom = format!("{}/bottom.txt", level_at(DEPTH - 1));
    assert!(dest.exists(&app(&bottom)).await, "the bottom file landed");
}

#[test]
#[ignore = "the recursive walk overflows a copy worker's stack (#1541)"]
fn a_two_hundred_level_tree_copies_on_a_copy_workers_stack() {
    let (result, dest, events) = on_a_copy_worker(|| async {
        let source = deep_source().await;
        let dest = no_links_dest();
        let engine = TestOperations::new();
        let events = Arc::new(CollectorEventSink::new());
        let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
        let sources = crate::testing::app_paths_of(&[PathBuf::from("/deep")]);
        let result = copy_volumes_with_progress(
            events.clone(),
            "op-deep-copy",
            &state,
            source,
            &sources,
            Arc::clone(&dest) as Arc<dyn Volume>,
            &app("/"),
            &VolumeCopyConfig::default(),
        )
        .await;
        (result, dest, events)
    });

    assert!(result.is_ok(), "{result:?}");
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(holds_the_whole_tree(&dest));
    let complete = events.complete.lock_ignore_poison();
    assert_eq!(complete[0].files_processed, DEPTH + 1);
}

#[test]
#[ignore = "the recursive walk overflows a copy worker's stack (#1541)"]
fn a_two_hundred_level_tree_moves_on_a_copy_workers_stack() {
    let (result, source, dest) = on_a_copy_worker(|| async {
        let source = deep_source().await;
        let dest = no_links_dest();
        let engine = TestOperations::new();
        let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
        let sources = crate::testing::app_paths_of(&[PathBuf::from("/deep")]);
        let result = move_volumes_with_progress(
            Arc::new(CollectorEventSink::new()),
            "op-deep-move",
            &state,
            Arc::clone(&source) as Arc<dyn Volume>,
            &sources,
            Arc::clone(&dest) as Arc<dyn Volume>,
            &app("/"),
            &VolumeCopyConfig::default(),
        )
        .await;
        (result, source, dest)
    });

    assert!(result.is_ok(), "{result:?}");
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        holds_the_whole_tree(&dest).await;
        assert!(
            !source.exists(&app("/deep")).await,
            "the sweep took the moved tree out of the source"
        );
    });
}
