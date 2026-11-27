//! A local move's folder merge (`merge_move_directory`) holds the deepest tree
//! macOS can name on the 2 MiB blocking-pool thread it runs on (#1541):
//! `crate::local_deep_tree_tests` says why these walks may recurse.
#![cfg(unix)] // A path this deep outgrows a Windows path without the long-path prefix.

use std::sync::Arc;

use super::test_support::make_state;
use super::{MoveTransaction, merge_move_directory};
use crate::conflict::ApplyToAll;
use crate::event_sinks::CollectorEventSink;
use crate::local_deep_tree_tests::{DEPTH, WALK_STACK, deep_tree, on_a_thread};
use crate::locked_sources::LockedPlan;
use crate::testing::TestOperations;
use crate::types::WriteOperationConfig;

/// Every level clashes with a folder the destination holds, so the merge goes
/// down the whole spine and moves the bottom file in beside the one there.
#[test]
fn a_local_merge_walks_the_deepest_tree_macos_can_name_on_its_threads_stack() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let source_top = deep_tree(source.path());
    let dest_top = dest.path().join("deep");
    let dest_bottom = dest_top.join("d/".repeat(DEPTH));
    std::fs::create_dir_all(&dest_bottom).unwrap();
    std::fs::write(dest_bottom.join("kept.txt"), b"kept").unwrap();
    let merge_into = dest_top.clone();

    let merged = on_a_thread(WALK_STACK, move || {
        let state = make_state(&TestOperations::new(), 200);
        let mut move_tx = MoveTransaction::new();
        merge_move_directory(
            &source_top,
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
        )
    });

    assert!(merged.is_ok(), "{merged:?}");
    assert_eq!(std::fs::read(dest_bottom.join("bottom.txt")).unwrap(), b"bottom");
    assert_eq!(std::fs::read(dest_bottom.join("kept.txt")).unwrap(), b"kept");
}
