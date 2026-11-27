//! A local move's folder merge (`merge_move_directory`) on the deepest tree
//! macOS can name, on the 2 MiB blocking-pool thread it runs on and with few
//! open files to spare (#1541): it keeps its levels on the heap and each level's
//! listing drained, `crate::local_deep_tree_tests` says why. These pin what the
//! recursion did at depth too: the order the spine leaves, and a failure at the
//! bottom leaving the source whole with its locks back on.
#![cfg(unix)] // A path this deep outgrows a Windows path without the long-path prefix.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::test_support::{make_state, run_same_fs_move};
use super::{MoveStep, MoveTransaction, merge_move_directory};
use crate::conflict::ApplyToAll;
use crate::event_sinks::CollectorEventSink;
use crate::local_deep_tree_tests::{DEPTH, WALK_STACK, deep_tree, on_a_thread};
use crate::locked_sources::LockedPlan;
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, WriteOperationConfig};

/// `top`, then each of its `DEPTH` levels, outermost first.
fn spine(top: &Path) -> Vec<PathBuf> {
    let mut levels = vec![top.to_path_buf()];
    for _ in 0..DEPTH {
        let next = levels[levels.len() - 1].join("d");
        levels.push(next);
    }
    levels
}

/// `dir/deep` and its levels, with `kept.txt` at the bottom: the destination
/// the source's spine merges into at every level. Answers the bottom folder.
fn dest_spine(dir: &Path) -> PathBuf {
    let bottom = dir.join("deep").join("d/".repeat(DEPTH));
    std::fs::create_dir_all(&bottom).unwrap();
    std::fs::write(bottom.join("kept.txt"), b"kept").unwrap();
    bottom
}

/// Every level clashes with a folder the destination holds, so the merge goes
/// down the whole spine and moves the bottom file in beside the one there. Each
/// emptied source level goes once its children have, deepest first, and the
/// ledger records them in that order, so a reversal pops them outermost first
/// and every parent is back before the children above it return into it.
#[test]
fn a_local_merge_walks_the_deepest_tree_macos_can_name_and_removes_its_spine_deepest_first() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let source_top = deep_tree(source.path());
    let dest_bottom = dest_spine(dest.path());
    let merge_into = dest.path().join("deep");
    let top = source_top.clone();

    let (merged, mut move_tx) = on_a_thread(WALK_STACK, move || {
        let state = make_state(&TestOperations::new(), 200);
        let mut move_tx = MoveTransaction::new();
        let merged = merge_move_directory(
            &top,
            &merge_into,
            &WriteOperationConfig::default(),
            &*Arc::new(CollectorEventSink::new()),
            "op-deep-merge",
            &state,
            &mut ApplyToAll::default(),
            &mut move_tx,
            &mut 0,
            &mut None,
            &LockedPlan::none(),
        );
        (merged, move_tx)
    });

    assert!(merged.is_ok(), "{merged:?}");
    assert_eq!(std::fs::read(dest_bottom.join("bottom.txt")).unwrap(), b"bottom");
    assert_eq!(std::fs::read(dest_bottom.join("kept.txt")).unwrap(), b"kept");
    assert!(!source_top.exists(), "every emptied source level went");
    let mut removed = Vec::new();
    while let Some(step) = move_tx.pop() {
        match step {
            MoveStep::RemovedSourceDir(dir) => removed.push(dir),
            MoveStep::Renamed(_) => assert!(removed.len() == DEPTH + 1, "the file renamed before any level went"),
        }
    }
    assert_eq!(
        removed,
        spine(&source_top),
        "popped outermost first, so recorded deepest first"
    );
}

/// The whole same-drive move: its Locked-items scan of what merges
/// (`LockedScan::of_renames`) walks the spine too, before the merge does.
#[test]
fn a_same_drive_move_merges_the_deepest_tree_macos_can_name() {
    let work = tempfile::tempdir().unwrap();
    let source_top = deep_tree(&work.path().join("src"));
    let dest_bottom = dest_spine(&work.path().join("dst"));
    let dst = work.path().join("dst");

    let moved = on_a_thread(WALK_STACK, move || {
        run_same_fs_move(
            &TestOperations::new(),
            &[source_top],
            &dst,
            ConflictResolution::Stop,
            "op-deep-same-fs-move",
        )
        .map(|_| ())
    });

    assert!(moved.is_ok(), "{moved:?}");
    assert_eq!(std::fs::read(dest_bottom.join("bottom.txt")).unwrap(), b"bottom");
    assert!(!work.path().join("src/deep").exists());
}

/// macOS's user Locked flag (`chflags uchg`).
#[cfg(target_os = "macos")]
fn set_locked(path: &Path, locked: bool) {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let flags = if locked { libc::UF_IMMUTABLE } else { 0 };
    // SAFETY: `c_path` is a valid NUL-terminated path for the call's duration.
    assert_eq!(unsafe { libc::chflags(c_path.as_ptr(), flags) }, 0, "chflags {path:?}");
}

#[cfg(target_os = "macos")]
fn is_locked(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;
    std::fs::symlink_metadata(path).unwrap().st_flags() & libc::UF_IMMUTABLE != 0
}

/// The merge fails at the bottom (the destination's deepest folder refuses the
/// file), with three Locked levels high on the source's spine that the move
/// agreed to unlock: nothing of the source moves or goes, and each Locked level
/// is Locked again where it stands.
#[cfg(target_os = "macos")]
#[test]
fn a_merge_failing_at_the_bottom_leaves_the_deepest_source_whole_and_locked_again() {
    use std::os::unix::fs::PermissionsExt;
    let work = tempfile::tempdir().unwrap();
    let source_top = deep_tree(&work.path().join("src"));
    let dest_bottom = dest_spine(&work.path().join("dst"));
    let source_levels = spine(&source_top);
    let locked: Vec<PathBuf> = source_levels[5..8].to_vec();
    for level in &locked {
        set_locked(level, true);
    }
    std::fs::set_permissions(&dest_bottom, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (sources, dst) = (vec![source_top.clone()], work.path().join("dst"));

    let moved = on_a_thread(WALK_STACK, move || {
        run_same_fs_move(
            &TestOperations::new(),
            &sources,
            &dst,
            ConflictResolution::Stop,
            "op-deep-merge-fails",
        )
        .map(|_| ())
    });

    let relocked: Vec<bool> = locked.iter().map(|level| is_locked(level)).collect();
    // Let the temp folders go.
    std::fs::set_permissions(&dest_bottom, std::fs::Permissions::from_mode(0o755)).unwrap();
    for level in &locked {
        set_locked(level, false);
    }
    assert!(moved.is_err(), "the bottom refused the file");
    assert_eq!(relocked, [true, true, true], "every Locked level is Locked again");
    let source_bottom = source_levels[DEPTH].join("bottom.txt");
    assert_eq!(std::fs::read(source_bottom).unwrap(), b"bottom", "the source is whole");
    assert!(!dest_bottom.join("bottom.txt").exists());
}
