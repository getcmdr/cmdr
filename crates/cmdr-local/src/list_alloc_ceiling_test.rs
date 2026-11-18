//! What one listed entry costs the local disk in allocations, as a ceiling: a change to
//! how a listing builds its paths (the typed app path builds one exact-capacity `String`
//! where today `PathBuf::join` then `to_string_lossy` build two) may lower it, ❌ never
//! raise it. The in-memory volume's twin is `cmdr-fs`'s `list_alloc_ceiling_test.rs`,
//! and `allocations_for_one_entry` says why it's one entry over none.

#![cfg(unix)]

use cmdr_fs::testing::TestDir;
use cmdr_fs::testing::alloc_count::{CountingAllocator, allocations_during, allocations_for_one_entry};
use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;

use super::LocalPosixVolume;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Allocations for one listed file over an empty folder through
/// `LocalPosixVolume::list_directory`, measured 2026-11-18 (`cargo nextest`, debug build).
/// Per OS because the listers differ: `getattrlistbulk` on macOS, `read_dir` plus `lstat`
/// on Linux.
#[cfg(target_os = "macos")]
const LOCAL_CEILING: u64 = 9;
#[cfg(not(target_os = "macos"))]
const LOCAL_CEILING: u64 = 13;

#[test]
fn a_local_listing_costs_at_most_its_pinned_allocations_per_entry() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .build()
        .expect("a runtime");
    let empty = TestDir::new("alloc_ceiling_empty");
    let one = TestDir::new("alloc_ceiling_one");
    std::fs::write(one.join("file_0.txt"), b"x").expect("a file");
    let per_entry = allocations_for_one_entry(|count| {
        let dir = if count == 0 { &empty } else { &one };
        let volume = LocalPosixVolume::local_folder("Test", dir.to_path_buf(), VolumeHost::detached());
        let (listed, allocations) = allocations_during(|| runtime.block_on(volume.list_directory(dir, None)));
        assert_eq!(listed.expect("lists").len() as u64, count);
        allocations
    });
    assert!(
        per_entry <= LOCAL_CEILING,
        "a local listing's allocations per entry went over the ceiling: {per_entry} > {LOCAL_CEILING}"
    );
    assert!(per_entry > 0, "the counter is live: {per_entry}");
}
