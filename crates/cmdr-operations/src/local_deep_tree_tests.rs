//! The local engines' synchronous walks hold the deepest tree macOS can name on
//! the 2 MiB threads they run on (#1541).
//!
//! A local walk reads full paths, so macOS's 1,024-byte `PATH_MAX` stops it near
//! 511 one-letter levels with `ENAMETOOLONG` (an error, never an overflow). The
//! scan and the dry run run on `run_cancellable_scoped`'s `std::thread`, the
//! move's merge on tokio's blocking pool, 2 MiB each. A walk that recursed per
//! level overflowed that short of this depth in a debug build, so each keeps its
//! depth on the heap, and one that starts nesting again aborts this test binary
//! here. The measurement: `transfer/volume/DETAILS.md` § "The walk keeps its
//! depth on the heap".
#![cfg(unix)] // A path this deep outgrows a Windows path without the long-path prefix.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::WriteOperationType;

/// The stack each walk's thread really has.
pub(crate) const WALK_STACK: usize = 2 * 1024 * 1024;

/// As deep as a temp folder leaves room for under `PATH_MAX`: each level adds
/// two bytes (`/d`), and the source and destination each sit below a temp
/// folder of ~60 bytes.
pub(crate) const DEPTH: usize = 460;

/// `dir/deep/d/d/…` (`DEPTH` levels below `deep`), with one file at the bottom;
/// answers `dir/deep`.
pub(crate) fn deep_tree(dir: &Path) -> PathBuf {
    let top = dir.join("deep");
    let bottom = top.join("d/".repeat(DEPTH));
    std::fs::create_dir_all(&bottom).unwrap();
    std::fs::write(bottom.join("bottom.txt"), b"bottom").unwrap();
    top
}

/// Runs `walk` on a thread of `stack` bytes.
pub(crate) fn on_a_thread<T: Send + 'static>(stack: usize, walk: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(walk)
        .unwrap()
        .join()
        .expect("the walk ends without overflowing its stack")
}

/// The local copy's and move's scan (`scan_walker.rs::walk_dir_recursive`).
#[test]
fn the_local_scan_walks_the_deepest_tree_macos_can_name_on_its_threads_stack() {
    let dir = tempfile::tempdir().unwrap();
    let top = deep_tree(dir.path());

    let scanned = on_a_thread(WALK_STACK, move || {
        let state = Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(50)));
        crate::scan::scan_sources(
            &[top],
            &state,
            &CollectorEventSink::new(),
            "op-deep-scan",
            WriteOperationType::Copy,
        )
    });

    let scanned = scanned.expect("the scan finishes");
    assert_eq!(scanned.files.len(), 1);
    assert_eq!(scanned.dirs.len(), DEPTH + 1, "every level, `deep` included");
}

/// The conflict dry run (`scan_dry_run.rs::dry_run_scan_recursive`), against a
/// destination holding the same tree, so every level is walked on both sides.
#[test]
fn the_local_dry_run_walks_the_deepest_tree_macos_can_name_on_its_threads_stack() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let top = deep_tree(source.path());
    deep_tree(dest.path());
    let dest_dir = dest.path().to_path_buf();

    let dry_run = on_a_thread(WALK_STACK, move || {
        let state = Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(50)));
        crate::scan_dry_run::dry_run_scan(
            &[top],
            &dest_dir,
            &state,
            &CollectorEventSink::new(),
            "op-deep-dry-run",
            WriteOperationType::Copy,
            Duration::from_secs(3600),
        )
    });

    assert!(dry_run.is_ok(), "the dry run finishes");
}
