//! What the merge walk does ACROSS levels, call by call (#1541).
//!
//! The per-level contracts have suites of their own (`merge_tests.rs`,
//! `copy_rollback_tests.rs`, `refused_name_tests.rs`, `link_cycle_tests.rs`, …).
//! These pin what only the walk's SHAPE decides: the order it descends in, which
//! of a child's outcomes its parent level acts on, and where a stop deep in the
//! tree leaves the rest of it. Each cell runs the real pipeline over one nested
//! fixture and photographs every call the walk made of each volume, in order,
//! through `FaultyVolume::journal` on a serial window (width 1), where that order
//! is fixed.
//!
//! The fixture, `/t` on both sides (`d` marks what the destination already holds):
//!
//! ```text
//! /t/a/            d   a pre-existing level: listed, merged into
//!   deep/          d     a pre-existing level two down
//!     x.txt        d       a clash two levels down
//!     y.txt
//!     z/                   an empty fresh folder
//!   a1.txt
//!   a2.txt         d     a clash one level down
//!   keep.txt       d     the destination's own (no source twin)
//! /t/b/            d   a FILE at the destination: a folder landing on a leaf
//!   b1.txt
//! /t/c/                a fresh subtree: created, never listed
//!   cc/c2.txt
//!   c1.txt
//! /t/m.txt         d   a clash at the folder's own level
//! /t/z.txt
//! ```

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::super::conflict_responder_test_support::ConflictResponderSink;
use super::copy::copy_volumes_with_progress;
use super::faulty_volume::{FaultyOp, FaultyVolume};
use super::move_cross::move_volumes_with_progress;
use super::transfer_error::WriteFailure;
use crate::event_sinks::CollectorEventSink;
use crate::operation_intent::OperationIntent;
use crate::state::WriteOperationState;
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, VolumeCopyConfig, WriteOperationPhase};

fn app(path: &str) -> AppPathBuf {
    AppPath::from_normalized(path).expect("a canonical path").to_owned()
}

async fn seeded() -> (Arc<InMemoryVolume>, Arc<InMemoryVolume>) {
    let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    let dest = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    for dir in ["/t", "/t/a", "/t/a/deep", "/t/a/deep/z", "/t/b", "/t/c", "/t/c/cc"] {
        source.create_directory(&app(dir)).await.unwrap();
    }
    for (file, bytes) in [
        ("/t/a/deep/x.txt", "x-src"),
        ("/t/a/deep/y.txt", "y-src"),
        ("/t/a/a1.txt", "a1-src"),
        ("/t/a/a2.txt", "a2-src"),
        ("/t/b/b1.txt", "b1-src"),
        ("/t/c/cc/c2.txt", "c2-src"),
        ("/t/c/c1.txt", "c1-src"),
        ("/t/m.txt", "m-src"),
        ("/t/z.txt", "z-src"),
    ] {
        source.create_file(&app(file), bytes.as_bytes()).await.unwrap();
    }
    for dir in ["/t", "/t/a", "/t/a/deep"] {
        dest.create_directory(&app(dir)).await.unwrap();
    }
    for (file, bytes) in [
        ("/t/a/deep/x.txt", "x-dst"),
        ("/t/a/a2.txt", "a2-dst"),
        ("/t/a/keep.txt", "keep-dst"),
        ("/t/b", "b-dst"),
        ("/t/m.txt", "m-dst"),
    ] {
        dest.create_file(&app(file), bytes.as_bytes()).await.unwrap();
    }
    (source, dest)
}

/// What the destination holds before any cell runs.
const UNTOUCHED_DEST: &[(&str, &str)] = &[
    ("/t/a/", ""),
    ("/t/a/a2.txt", "a2-dst"),
    ("/t/a/deep/", ""),
    ("/t/a/deep/x.txt", "x-dst"),
    ("/t/a/keep.txt", "keep-dst"),
    ("/t/b", "b-dst"),
    ("/t/m.txt", "m-dst"),
];

/// Both sides journaled; the destination reports `width`, which sizes the
/// window (1: serial).
struct Rig {
    source: Arc<FaultyVolume<InMemoryVolume>>,
    dest: Arc<FaultyVolume<InMemoryVolume>>,
    source_inner: Arc<InMemoryVolume>,
    dest_inner: Arc<InMemoryVolume>,
}

async fn rig(
    width: usize,
    dest_fault: impl FnOnce(FaultyVolume<InMemoryVolume>) -> FaultyVolume<InMemoryVolume>,
) -> Rig {
    let (source_inner, dest_inner) = seeded().await;
    Rig {
        source: FaultyVolume::wrapping(Arc::clone(&source_inner)).arc(),
        dest: dest_fault(FaultyVolume::wrapping(Arc::clone(&dest_inner)).with_max_concurrent_ops(width)).arc(),
        source_inner,
        dest_inner,
    }
}

fn config(policy: ConflictResolution) -> VolumeCopyConfig {
    VolumeCopyConfig {
        progress_interval_ms: 0,
        conflict_resolution: policy,
        ..VolumeCopyConfig::default()
    }
}

fn state() -> Arc<WriteOperationState> {
    Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(50)))
}

async fn copy(
    rig: &Rig,
    state: &Arc<WriteOperationState>,
    events: Arc<ConflictResponderSink>,
    policy: ConflictResolution,
) -> Result<(), WriteFailure> {
    copy_volumes_with_progress(
        events,
        "op-walk",
        state,
        Arc::clone(&rig.source) as Arc<dyn Volume>,
        &crate::testing::app_paths_of(&[PathBuf::from("/t")]),
        Arc::clone(&rig.dest) as Arc<dyn Volume>,
        &app("/"),
        &config(policy),
    )
    .await
}

/// The journal as `"<Op> <path>"` lines, only the ops named, with every
/// generated scratch suffix (`x.txt.cmdr-tmp-<uuid>`, a swap's
/// `.cmdr-temp-<uuid>`) reduced to its kind.
fn shape(volume: &FaultyVolume<InMemoryVolume>, ops: &[FaultyOp]) -> Vec<String> {
    volume
        .journal()
        .iter()
        .filter(|(op, _)| ops.contains(op))
        .map(|(op, path)| format!("{op:?} {}", without_scratch(path.as_str())))
        .collect()
}

fn without_scratch(path: &str) -> String {
    match path.find(".cmdr-") {
        Some(at) => {
            let (kept, scratch) = path.split_at(at + ".cmdr-".len());
            format!("{kept}{}-*", scratch.split('-').next().unwrap_or_default())
        }
        None => path.to_owned(),
    }
}

/// What a walk asks of the destination that changes it, or reads a level.
const DEST_CALLS: &[FaultyOp] = &[
    FaultyOp::CreateDirectory,
    FaultyOp::ListDirectory,
    FaultyOp::Rename,
    FaultyOp::RenameFrom,
    FaultyOp::Delete,
];

/// Every file under `root` with its bytes, and every folder (as `<path>/`).
async fn tree(volume: &InMemoryVolume, root: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut pending = vec![app(root)];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = volume.list_directory(&dir, None).await else {
            continue;
        };
        for entry in entries {
            let path = app(&entry.path);
            if entry.is_directory {
                out.insert(format!("{}/", path.as_str()), String::new());
                pending.push(path);
            } else {
                let bytes = read(volume, &path).await;
                out.insert(path.as_str().to_owned(), bytes);
            }
        }
    }
    out
}

async fn read(volume: &InMemoryVolume, path: &AppPath) -> String {
    let mut stream = volume.open_read_stream(path).await.unwrap();
    let mut out = Vec::new();
    while let Some(Ok(chunk)) = stream.next_chunk().await {
        out.extend_from_slice(&chunk);
    }
    String::from_utf8(out).unwrap()
}

fn tree_of(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(path, bytes)| ((*path).to_owned(), (*bytes).to_owned()))
        .collect()
}

fn lines(expected: &[&str]) -> Vec<String> {
    expected.iter().map(|line| (*line).to_owned()).collect()
}

fn prompts(events: &CollectorEventSink) -> Vec<String> {
    events
        .conflicts
        .lock_ignore_poison()
        .iter()
        .map(|conflict| conflict.source_path.clone())
        .collect()
}

/// `(files, skipped, bytes)` off the one completion event.
fn completed(events: &CollectorEventSink) -> (usize, usize, u64) {
    let complete = events.complete.lock_ignore_poison();
    assert_eq!(complete.len(), 1, "one completion");
    (
        complete[0].files_processed,
        complete[0].files_skipped,
        complete[0].bytes_processed,
    )
}

/// The copying events' counters only ever climb; answers the last one.
fn progress_climbs(events: &CollectorEventSink) -> (usize, u64) {
    let progress = events.progress.lock_ignore_poison();
    let copying: Vec<_> = progress
        .iter()
        .filter(|event| event.phase == WriteOperationPhase::Copying)
        .collect();
    for pair in copying.windows(2) {
        assert!(pair[1].files_done >= pair[0].files_done, "files went back: {pair:?}");
        assert!(pair[1].bytes_done >= pair[0].bytes_done, "bytes went back: {pair:?}");
    }
    let last = copying.last().expect("the copy reported progress");
    (last.files_done, last.bytes_done)
}

/// Every clash asked where the walk meets it, folders before files at each
/// level.
const EVERY_CLASH: &[&str] = &["/t/a/deep/x.txt", "/t/a/a2.txt", "/t/b", "/t/m.txt"];

/// The order the walker creates and lists the destination's levels in, the
/// same under any window: a level's prepare before its first child, and a
/// child folder's whole subtree before its next sibling. `/t/b` is created
/// once the clash moved the leaf there aside; `/t/c` is fresh, so never listed.
const WALKER_LEVELS: &[&str] = &[
    "CreateDirectory /",
    "ListDirectory /",
    "CreateDirectory /t",
    "ListDirectory /t",
    "CreateDirectory /t/a",
    "ListDirectory /t/a",
    "CreateDirectory /t/a/deep",
    "ListDirectory /t/a/deep",
    "CreateDirectory /t/a/deep/z",
    "CreateDirectory /t/b",
    "CreateDirectory /t/c",
    "CreateDirectory /t/c/cc",
];

/// The whole source landed, over every clash, beside the destination's own.
const OVERWRITTEN: &[(&str, &str)] = &[
    ("/t/a/", ""),
    ("/t/a/a1.txt", "a1-src"),
    ("/t/a/a2.txt", "a2-src"),
    ("/t/a/deep/", ""),
    ("/t/a/deep/x.txt", "x-src"),
    ("/t/a/deep/y.txt", "y-src"),
    ("/t/a/deep/z/", ""),
    ("/t/a/keep.txt", "keep-dst"),
    ("/t/b/", ""),
    ("/t/b/b1.txt", "b1-src"),
    ("/t/c/", ""),
    ("/t/c/c1.txt", "c1-src"),
    ("/t/c/cc/", ""),
    ("/t/c/cc/c2.txt", "c2-src"),
    ("/t/m.txt", "m-src"),
    ("/t/z.txt", "z-src"),
];

/// A serial walk with every clash answered Overwrite, one at a time: each
/// file lands before the walker moves to the next child, a swap's original
/// goes once its replacement landed, and the leaf a folder displaced goes last,
/// once the operation knows how it ended.
#[tokio::test]
async fn a_serial_walk_descends_in_tree_order_and_asks_each_clash_where_it_meets_it() {
    let rig = rig(1, |dest| dest).await;
    let state = state();
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Overwrite, false));

    let result = copy(&rig, &state, Arc::clone(&events), ConflictResolution::Stop).await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(prompts(&events.inner), lines(EVERY_CLASH));
    assert_eq!(
        shape(&rig.source, &[FaultyOp::ListDirectory]),
        lines(&[
            "ListDirectory /t",
            "ListDirectory /t/a",
            "ListDirectory /t/a/deep",
            "ListDirectory /t/a/deep/z",
            "ListDirectory /t/b",
            "ListDirectory /t/c",
            "ListDirectory /t/c/cc",
        ])
    );
    assert_eq!(
        shape(&rig.dest, DEST_CALLS),
        lines(&[
            "CreateDirectory /",
            "ListDirectory /",
            "CreateDirectory /t",
            "ListDirectory /t",
            "CreateDirectory /t/a",
            "ListDirectory /t/a",
            "CreateDirectory /t/a/deep",
            "ListDirectory /t/a/deep",
            "CreateDirectory /t/a/deep/z",
            "RenameFrom /t/a/deep/x.txt",
            "Rename /t/a/deep/x.txt.cmdr-temp-*",
            "RenameFrom /t/a/deep/x.txt.cmdr-tmp-*",
            "Rename /t/a/deep/x.txt",
            "Delete /t/a/deep/x.txt.cmdr-temp-*",
            "RenameFrom /t/a/deep/y.txt.cmdr-tmp-*",
            "Rename /t/a/deep/y.txt",
            "RenameFrom /t/a/a1.txt.cmdr-tmp-*",
            "Rename /t/a/a1.txt",
            "RenameFrom /t/a/a2.txt",
            "Rename /t/a/a2.txt.cmdr-temp-*",
            "RenameFrom /t/a/a2.txt.cmdr-tmp-*",
            "Rename /t/a/a2.txt",
            "Delete /t/a/a2.txt.cmdr-temp-*",
            "RenameFrom /t/b",
            "Rename /t/b.cmdr-temp-*",
            "CreateDirectory /t/b",
            "RenameFrom /t/b/b1.txt.cmdr-tmp-*",
            "Rename /t/b/b1.txt",
            "CreateDirectory /t/c",
            "CreateDirectory /t/c/cc",
            "RenameFrom /t/c/cc/c2.txt.cmdr-tmp-*",
            "Rename /t/c/cc/c2.txt",
            "RenameFrom /t/c/c1.txt.cmdr-tmp-*",
            "Rename /t/c/c1.txt",
            "RenameFrom /t/m.txt",
            "Rename /t/m.txt.cmdr-temp-*",
            "RenameFrom /t/m.txt.cmdr-tmp-*",
            "Rename /t/m.txt",
            "Delete /t/m.txt.cmdr-temp-*",
            "RenameFrom /t/z.txt.cmdr-tmp-*",
            "Rename /t/z.txt",
            "Delete /t/b.cmdr-temp-*",
        ])
    );
    assert_eq!(tree(&rig.dest_inner, "/t").await, tree_of(OVERWRITTEN));
    assert_eq!(completed(&events.inner), (9, 0, 50));
    assert_eq!(progress_climbs(&events.inner), (9, 50));
}

/// A wide window overlaps the BYTES only: the walker still creates, lists, and
/// asks in the serial walk's order, and the tree ends the same.
#[tokio::test]
async fn a_wide_window_walks_and_asks_in_the_serial_order() {
    let rig = rig(8, |dest| dest).await;
    let state = state();
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Overwrite, false));

    let result = copy(&rig, &state, Arc::clone(&events), ConflictResolution::Stop).await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(prompts(&events.inner), lines(EVERY_CLASH));
    assert_eq!(
        shape(&rig.dest, &[FaultyOp::CreateDirectory, FaultyOp::ListDirectory]),
        lines(WALKER_LEVELS)
    );
    assert_eq!(tree(&rig.dest_inner, "/t").await, tree_of(OVERWRITTEN));
    assert_eq!(completed(&events.inner), (9, 0, 50));
}

/// Skip all at the first clash: every later clash, at any depth and of either
/// kind (the folder on a leaf too), is a counted skip with no prompt and no
/// write, and the walk goes on past each.
#[tokio::test]
async fn skip_all_at_the_first_clash_skips_every_later_one_at_any_depth() {
    let rig = rig(1, |dest| dest).await;
    let state = state();
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, true));

    let result = copy(&rig, &state, Arc::clone(&events), ConflictResolution::Stop).await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(prompts(&events.inner), lines(&["/t/a/deep/x.txt"]));
    assert_eq!(
        shape(&rig.dest, DEST_CALLS),
        lines(&[
            "CreateDirectory /",
            "ListDirectory /",
            "CreateDirectory /t",
            "ListDirectory /t",
            "CreateDirectory /t/a",
            "ListDirectory /t/a",
            "CreateDirectory /t/a/deep",
            "ListDirectory /t/a/deep",
            "CreateDirectory /t/a/deep/z",
            "RenameFrom /t/a/deep/y.txt.cmdr-tmp-*",
            "Rename /t/a/deep/y.txt",
            "RenameFrom /t/a/a1.txt.cmdr-tmp-*",
            "Rename /t/a/a1.txt",
            "CreateDirectory /t/c",
            "CreateDirectory /t/c/cc",
            "RenameFrom /t/c/cc/c2.txt.cmdr-tmp-*",
            "Rename /t/c/cc/c2.txt",
            "RenameFrom /t/c/c1.txt.cmdr-tmp-*",
            "Rename /t/c/c1.txt",
            "RenameFrom /t/z.txt.cmdr-tmp-*",
            "Rename /t/z.txt",
        ])
    );
    assert_eq!(
        tree(&rig.dest_inner, "/t").await,
        tree_of(&[
            ("/t/a/", ""),
            ("/t/a/a1.txt", "a1-src"),
            ("/t/a/a2.txt", "a2-dst"),
            ("/t/a/deep/", ""),
            ("/t/a/deep/x.txt", "x-dst"),
            ("/t/a/deep/y.txt", "y-src"),
            ("/t/a/deep/z/", ""),
            ("/t/a/keep.txt", "keep-dst"),
            ("/t/b", "b-dst"),
            ("/t/c/", ""),
            ("/t/c/c1.txt", "c1-src"),
            ("/t/c/cc/", ""),
            ("/t/c/cc/c2.txt", "c2-src"),
            ("/t/m.txt", "m-dst"),
            ("/t/z.txt", "z-src"),
        ])
    );
    assert_eq!(completed(&events.inner), (9, 4, 28));
}

/// The levels and the swap the walk had made by the time it reached
/// `/t/a/deep/y.txt`, the deepest level's second file.
const UP_TO_Y: &[&str] = &[
    "CreateDirectory /",
    "ListDirectory /",
    "CreateDirectory /t",
    "ListDirectory /t",
    "CreateDirectory /t/a",
    "ListDirectory /t/a",
    "CreateDirectory /t/a/deep",
    "ListDirectory /t/a/deep",
    "CreateDirectory /t/a/deep/z",
];

/// A landing that fails two levels down ends the walk at that file: nothing
/// after it in tree order starts, the failure names it, and what landed before
/// it stays (no rollback was asked for).
#[tokio::test]
async fn a_failure_two_levels_down_stops_the_walk_at_that_file() {
    let rig = rig(1, |dest| {
        dest.failing_call_at(
            FaultyOp::Rename,
            "/t/a/deep/y.txt",
            1,
            VolumeError::IoError {
                message: "the share said no".into(),
                raw_os_error: None,
            },
        )
    })
    .await;
    let state = state();
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, false));

    let result = copy(&rig, &state, Arc::clone(&events), ConflictResolution::Overwrite).await;

    assert!(rig.dest.fault_fired(FaultyOp::Rename));
    let failure = result.expect_err("the failed landing fails the copy");
    assert_eq!(
        format!("{:?}", failure.error),
        r#"IoError { path: "/t/a/deep/y.txt", message: "the share said no" }"#
    );
    let mut expected = lines(UP_TO_Y);
    expected.extend(lines(&[
        "RenameFrom /t/a/deep/x.txt",
        "Rename /t/a/deep/x.txt.cmdr-temp-*",
        "RenameFrom /t/a/deep/x.txt.cmdr-tmp-*",
        "Rename /t/a/deep/x.txt",
        "Delete /t/a/deep/x.txt.cmdr-temp-*",
        "RenameFrom /t/a/deep/y.txt.cmdr-tmp-*",
        "Rename /t/a/deep/y.txt",
        "Delete /t/a/deep/y.txt.cmdr-tmp-*",
    ]));
    assert_eq!(shape(&rig.dest, DEST_CALLS), expected);
    let mut landed = tree_of(UNTOUCHED_DEST);
    landed.insert("/t/a/deep/x.txt".into(), "x-src".into());
    landed.insert("/t/a/deep/z/".into(), String::new());
    assert_eq!(tree(&rig.dest_inner, "/t").await, landed);
}

/// Cancel on a clash two levels down: the walk stops there, keeps every folder
/// it made, and starts nothing after it.
#[tokio::test]
async fn a_cancel_at_a_deep_clash_keeps_what_it_made_and_starts_nothing_after_it() {
    let rig = rig(1, |dest| dest).await;
    let engine = TestOperations::new();
    let op = engine.register_state("op-walk", state());
    let events = Arc::new(ConflictResponderSink::cancelling(&engine, op.state(), op.id().as_str()));

    let result = copy(&rig, op.state(), Arc::clone(&events), ConflictResolution::Stop).await;

    assert_eq!(
        format!("{result:?}"),
        r#"Err(WriteFailure { error: Cancelled { message: "Operation cancelled by user" } })"#
    );
    assert_eq!(prompts(&events.inner), lines(&["/t/a/deep/x.txt"]));
    assert_eq!(shape(&rig.dest, DEST_CALLS), lines(UP_TO_Y));
    let mut kept = tree_of(UNTOUCHED_DEST);
    kept.insert("/t/a/deep/z/".into(), String::new());
    assert_eq!(tree(&rig.dest_inner, "/t").await, kept);
    assert_eq!(events.inner.cancelled.lock_ignore_poison().len(), 1);
}

/// Rollback asked at the second clash (one level up from the first): the walk
/// stops, and the rollback takes out the files it landed, newest first, then
/// the folder it made, and nothing the destination had.
#[tokio::test]
async fn a_rollback_mid_walk_undoes_it_newest_first() {
    let rig = rig(1, |dest| dest).await;
    let state = state();
    let intent = state.intent().clone();
    let asked = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(
        ConflictResponderSink::new(&state, ConflictResolution::Skip, false).on_prompt(move || {
            if asked.fetch_add(1, Ordering::SeqCst) == 1 {
                intent.set_for_test(OperationIntent::RollingBack);
            }
        }),
    );

    let result = copy(&rig, &state, Arc::clone(&events), ConflictResolution::Stop).await;

    assert_eq!(
        format!("{result:?}"),
        r#"Err(WriteFailure { error: Cancelled { message: "Operation cancelled by user" } })"#
    );
    assert_eq!(prompts(&events.inner), lines(&["/t/a/deep/x.txt", "/t/a/a2.txt"]));
    let mut expected = lines(UP_TO_Y);
    expected.extend(lines(&[
        "RenameFrom /t/a/deep/y.txt.cmdr-tmp-*",
        "Rename /t/a/deep/y.txt",
        "RenameFrom /t/a/a1.txt.cmdr-tmp-*",
        "Rename /t/a/a1.txt",
        "Delete /t/a/a1.txt",
        "Delete /t/a/deep/y.txt",
        "ListDirectory /t/a/deep/z",
        "Delete /t/a/deep/z",
    ]));
    assert_eq!(shape(&rig.dest, DEST_CALLS), expected);
    assert_eq!(tree(&rig.dest_inner, "/t").await, tree_of(UNTOUCHED_DEST));
}

/// A move over the nested tree with clashes skipped: the walk never enters
/// the folder it skipped, and the sweep takes out what it carried and every
/// folder that empties, and leaves each skipped item with the folders above it.
#[tokio::test]
async fn a_move_sweeps_what_the_walk_carried_and_keeps_its_skips() {
    let rig = rig(1, |dest| dest).await;
    let state = state();

    let result = move_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-walk-move",
        &state,
        Arc::clone(&rig.source) as Arc<dyn Volume>,
        &crate::testing::app_paths_of(&[PathBuf::from("/t")]),
        Arc::clone(&rig.dest) as Arc<dyn Volume>,
        &app("/"),
        &config(ConflictResolution::Skip),
    )
    .await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        shape(&rig.source, &[FaultyOp::ListDirectory, FaultyOp::Delete]),
        lines(&[
            // The walk.
            "ListDirectory /t",
            "ListDirectory /t/a",
            "ListDirectory /t/a/deep",
            "ListDirectory /t/a/deep/z",
            "ListDirectory /t/c",
            "ListDirectory /t/c/cc",
            // The sweep.
            "ListDirectory /t",
            "ListDirectory /t/a",
            "ListDirectory /t/a/deep",
            "ListDirectory /t/a/deep/z",
            "Delete /t/a/deep/z",
            "Delete /t/a/deep/y.txt",
            "Delete /t/a/a1.txt",
            "ListDirectory /t/c",
            "ListDirectory /t/c/cc",
            "Delete /t/c/cc/c2.txt",
            "Delete /t/c/cc",
            "Delete /t/c/c1.txt",
            "Delete /t/c",
            "Delete /t/z.txt",
        ])
    );
    assert_eq!(
        tree(&rig.source_inner, "/t").await,
        tree_of(&[
            ("/t/a/", ""),
            ("/t/a/a2.txt", "a2-src"),
            ("/t/a/deep/", ""),
            ("/t/a/deep/x.txt", "x-src"),
            ("/t/b/", ""),
            ("/t/b/b1.txt", "b1-src"),
            ("/t/m.txt", "m-src"),
        ])
    );
    assert_eq!(
        tree(&rig.dest_inner, "/t").await,
        tree_of(&[
            ("/t/a/", ""),
            ("/t/a/a1.txt", "a1-src"),
            ("/t/a/a2.txt", "a2-dst"),
            ("/t/a/deep/", ""),
            ("/t/a/deep/x.txt", "x-dst"),
            ("/t/a/deep/y.txt", "y-src"),
            ("/t/a/deep/z/", ""),
            ("/t/a/keep.txt", "keep-dst"),
            ("/t/b", "b-dst"),
            ("/t/c/", ""),
            ("/t/c/c1.txt", "c1-src"),
            ("/t/c/cc/", ""),
            ("/t/c/cc/c2.txt", "c2-src"),
            ("/t/m.txt", "m-dst"),
            ("/t/z.txt", "z-src"),
        ])
    );
}

fn refused_name() -> VolumeError {
    VolumeError::InvalidName("STATUS_OBJECT_NAME_INVALID".into())
}

/// The destination refuses a folder's own name two levels down, and the
/// person answers Skip: the folder is left out whole (its files counted as
/// skipped), and the walk goes on at that folder's next sibling, the one place a
/// child's failure turns back into "go on".
#[tokio::test]
async fn a_refused_folder_name_skipped_two_levels_down_resumes_at_its_next_sibling() {
    let rig = rig(1, |dest| {
        dest.failing_call_at(FaultyOp::CreateDirectory, "/t/c/cc", 1, refused_name())
    })
    .await;
    let state = state();
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, false));

    let result = copy(&rig, &state, Arc::clone(&events), ConflictResolution::Skip).await;

    assert!(rig.dest.fault_fired(FaultyOp::CreateDirectory));
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        prompts(&events.inner),
        lines(&["/t/c/cc"]),
        "the refused name is asked about"
    );
    let mut expected = lines(UP_TO_Y);
    expected.extend(lines(&[
        "RenameFrom /t/a/deep/y.txt.cmdr-tmp-*",
        "Rename /t/a/deep/y.txt",
        "RenameFrom /t/a/a1.txt.cmdr-tmp-*",
        "Rename /t/a/a1.txt",
        "CreateDirectory /t/c",
        "CreateDirectory /t/c/cc",
        // The skipped folder's next sibling, then the walk's own next entry.
        "RenameFrom /t/c/c1.txt.cmdr-tmp-*",
        "Rename /t/c/c1.txt",
        "RenameFrom /t/z.txt.cmdr-tmp-*",
        "Rename /t/z.txt",
    ]));
    assert_eq!(shape(&rig.dest, DEST_CALLS), expected);
    let mut landed = tree_of(UNTOUCHED_DEST);
    for (path, bytes) in [
        ("/t/a/a1.txt", "a1-src"),
        ("/t/a/deep/y.txt", "y-src"),
        ("/t/a/deep/z/", ""),
        ("/t/c/", ""),
        ("/t/c/c1.txt", "c1-src"),
        ("/t/z.txt", "z-src"),
    ] {
        landed.insert(path.into(), bytes.into());
    }
    assert_eq!(tree(&rig.dest_inner, "/t").await, landed);
    // The four clashes, and the one file under the folder left out.
    assert_eq!(completed(&events.inner), (9, 5, 22));
}

/// The same refusal on a folder that took a same-named leaf's place (an
/// answered Overwrite set the leaf aside): no refused-name question there, since
/// the destination stored that name a moment ago, so the copy fails whole.
#[tokio::test]
async fn a_refusal_inside_a_folder_that_took_a_leafs_place_fails_the_copy_unasked() {
    let rig = rig(1, |dest| {
        dest.failing_call_at(FaultyOp::CreateDirectory, "/t/b", 1, refused_name())
    })
    .await;
    let state = state();
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Overwrite, false));

    let result = copy(&rig, &state, Arc::clone(&events), ConflictResolution::Stop).await;

    assert!(rig.dest.fault_fired(FaultyOp::CreateDirectory));
    assert_eq!(
        format!("{result:?}"),
        r#"Err(WriteFailure { error: InvalidName { path: "/t/b", message: "STATUS_OBJECT_NAME_INVALID" } })"#
    );
    // The clashes up to it, and no refused-name question after them.
    assert_eq!(
        prompts(&events.inner),
        lines(&["/t/a/deep/x.txt", "/t/a/a2.txt", "/t/b"])
    );
    let mut expected = lines(UP_TO_Y);
    expected.extend(lines(&[
        "RenameFrom /t/a/deep/x.txt",
        "Rename /t/a/deep/x.txt.cmdr-temp-*",
        "RenameFrom /t/a/deep/x.txt.cmdr-tmp-*",
        "Rename /t/a/deep/x.txt",
        "Delete /t/a/deep/x.txt.cmdr-temp-*",
        "RenameFrom /t/a/deep/y.txt.cmdr-tmp-*",
        "Rename /t/a/deep/y.txt",
        "RenameFrom /t/a/a1.txt.cmdr-tmp-*",
        "Rename /t/a/a1.txt",
        "RenameFrom /t/a/a2.txt",
        "Rename /t/a/a2.txt.cmdr-temp-*",
        "RenameFrom /t/a/a2.txt.cmdr-tmp-*",
        "Rename /t/a/a2.txt",
        "Delete /t/a/a2.txt.cmdr-temp-*",
        // The leaf set aside, the refused create, and the leaf put back.
        "RenameFrom /t/b",
        "Rename /t/b.cmdr-temp-*",
        "CreateDirectory /t/b",
        "RenameFrom /t/b.cmdr-temp-*",
        "Rename /t/b",
    ]));
    assert_eq!(shape(&rig.dest, DEST_CALLS), expected);
    let mut landed = tree_of(UNTOUCHED_DEST);
    for (path, bytes) in [
        ("/t/a/a1.txt", "a1-src"),
        ("/t/a/a2.txt", "a2-src"),
        ("/t/a/deep/x.txt", "x-src"),
        ("/t/a/deep/y.txt", "y-src"),
        ("/t/a/deep/z/", ""),
    ] {
        landed.insert(path.into(), bytes.into());
    }
    assert_eq!(tree(&rig.dest_inner, "/t").await, landed, "the leaf is back in place");
}

/// A move whose sweep can't delete a file two levels down: that file, and
/// every folder above it, stays in the source; the rest of what moved goes.
#[tokio::test]
async fn a_sweep_failure_two_levels_down_keeps_every_folder_above_it() {
    let (source_inner, dest_inner) = seeded().await;
    let rig = Rig {
        source: FaultyVolume::wrapping(Arc::clone(&source_inner))
            .failing_call_at(
                FaultyOp::Delete,
                "/t/a/deep/y.txt",
                1,
                VolumeError::IoError {
                    message: "the file is busy".into(),
                    raw_os_error: None,
                },
            )
            .arc(),
        dest: FaultyVolume::wrapping(Arc::clone(&dest_inner))
            .with_max_concurrent_ops(1)
            .arc(),
        source_inner,
        dest_inner,
    };
    let state = state();

    let result = move_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-walk-move-busy",
        &state,
        Arc::clone(&rig.source) as Arc<dyn Volume>,
        &crate::testing::app_paths_of(&[PathBuf::from("/t")]),
        Arc::clone(&rig.dest) as Arc<dyn Volume>,
        &app("/"),
        &config(ConflictResolution::Skip),
    )
    .await;

    assert!(rig.source.fault_fired(FaultyOp::Delete));
    assert_eq!(
        format!("{result:?}"),
        r#"Err(WriteFailure { error: SourceNotRemoved { path: "/t/a/deep/y.txt", landed_at: "/t", cause: IoError { path: "/t/a/deep/y.txt", message: "the file is busy" } } })"#
    );
    // No folder above the file that stayed is asked to go; the rest still does.
    assert_eq!(
        shape(&rig.source, &[FaultyOp::Delete]),
        lines(&[
            "Delete /t/a/deep/z",
            "Delete /t/a/deep/y.txt",
            "Delete /t/a/a1.txt",
            "Delete /t/c/cc/c2.txt",
            "Delete /t/c/cc",
            "Delete /t/c/c1.txt",
            "Delete /t/c",
            "Delete /t/z.txt",
        ])
    );
    assert_eq!(
        tree(&rig.source_inner, "/t").await,
        tree_of(&[
            ("/t/a/", ""),
            ("/t/a/a2.txt", "a2-src"),
            ("/t/a/deep/", ""),
            ("/t/a/deep/x.txt", "x-src"),
            ("/t/a/deep/y.txt", "y-src"),
            ("/t/b/", ""),
            ("/t/b/b1.txt", "b1-src"),
            ("/t/m.txt", "m-src"),
        ])
    );
}
