//! The oracle-aware subtree scan walks a tree of any depth on the stack a scan
//! preview really runs on (#1541): the host runtime's 2 MiB tokio worker. A scan
//! that nests a future (or a progress closure) per level aborts this test binary.

use cmdr_fs::os_junk_files::OsJunkFiles;
use cmdr_fs::volume::ListingProgress;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use super::scan_subtree_with_oracle;
use crate::deep_chain_test_support::{BOTTOM_SIZE, ChainVolume, TOP};
use crate::host::testing::PaneListings;

/// Far past what a frame-per-level scan survives on this stack, which no local
/// path limit caps on a remote.
const CHAIN: usize = 3_000;

#[test]
fn a_chain_thousands_of_levels_deep_scans_on_a_scan_workers_stack() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(2 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap();

    let (totals, last_progress) = runtime
        .block_on(runtime.spawn(async {
            let volume = ChainVolume::new(CHAIN);
            // No pane open, so every level reads the volume.
            let panes = PaneListings::new();
            let last = Mutex::new(ListingProgress::default());
            let on_progress = |p: ListingProgress| *last.lock().unwrap() = p;
            let totals = scan_subtree_with_oracle(
                &*panes,
                &volume,
                "chain",
                Path::new(TOP),
                &|| false,
                None,
                Some(&on_progress),
                None,
                &mut HashSet::new(),
                OsJunkFiles::Include,
            )
            .await;
            (totals, *last.lock().unwrap())
        }))
        .expect("the scan's task ends without a panic");

    let totals = totals.expect("the scan finishes");
    assert_eq!(totals.file_count, 1);
    assert_eq!(totals.dir_count, CHAIN, "every folder below the top one");
    assert_eq!(totals.total_bytes, BOTTOM_SIZE);
    assert_eq!(totals.per_path.len(), 1, "the top folder's one child");
    assert_eq!(
        (last_progress.files, last_progress.dirs, last_progress.bytes),
        (1, CHAIN, BOTTOM_SIZE),
        "the last report counts the whole subtree, every level's tally added up"
    );
}
