//! What one listed entry costs the local disk in allocations, as a ceiling: a change to
//! how a listing builds its paths (the typed app path builds one exact-capacity `String`
//! where today `PathBuf::join` then `to_string_lossy` build two) may lower it, ❌ never
//! raise it. The in-memory volume's twin is `cmdr-fs`'s `list_alloc_ceiling_test.rs`,
//! and `allocations_for_one_entry` says why it's one entry over none.

use cmdr_fs::testing::AppTestDir;
use cmdr_fs::testing::alloc_count::{CountingAllocator, allocations_during, allocations_for_one_entry};
use cmdr_fs::volume::Volume;

use crate::test_support::volume_at;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Allocations for one listed file over an empty folder through
/// `LocalVolume::list_directory`, measured 2026-11-18 (`cargo nextest`, debug build).
/// Per OS because the listers differ: `getattrlistbulk` on macOS, `read_dir` plus `lstat`
/// on Linux (and on Windows, whose own figure is still to be pinned).
#[cfg(target_os = "macos")]
const LOCAL_CEILING: u64 = 9;
#[cfg(not(target_os = "macos"))]
const LOCAL_CEILING: u64 = 13;

#[test]
#[cfg_attr(
    target_os = "windows",
    ignore = "unmeasured on Windows: pin LOCAL_CEILING from a Windows run first"
)]
fn a_local_listing_costs_at_most_its_pinned_allocations_per_entry() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .build()
        .expect("a runtime");
    let empty = AppTestDir::new("alloc_ceiling_empty");
    let one = AppTestDir::new("alloc_ceiling_one");
    std::fs::write(one.native().join("file_0.txt"), b"x").expect("a file");
    let per_entry = allocations_for_one_entry(|count| {
        let dir = if count == 0 { &empty } else { &one };
        let volume = volume_at(dir);
        let (listed, allocations) = allocations_during(|| runtime.block_on(volume.list_directory(volume.root(), None)));
        assert_eq!(listed.expect("lists").len() as u64, count);
        allocations
    });
    assert!(
        per_entry <= LOCAL_CEILING,
        "a local listing's allocations per entry went over the ceiling: {per_entry} > {LOCAL_CEILING}"
    );
    assert!(per_entry > 0, "the counter is live: {per_entry}");
}
