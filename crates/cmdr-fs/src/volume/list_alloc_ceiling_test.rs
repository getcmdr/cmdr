//! What one listed entry costs in allocations, as a ceiling: a change to how a listing
//! builds its paths (the typed app path builds one exact-capacity `String` where today
//! `PathBuf::join` then `to_string_lossy` build two) may lower it, ❌ never raise it.
//! The local disk's twin is `cmdr-local`'s `list_alloc_ceiling_test.rs`.

use std::path::Path;

use crate::entry::FileEntry;
use crate::testing::alloc_count::{CountingAllocator, allocations_during, allocations_for_one_entry};
use crate::volume::{InMemoryVolume, Volume};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Allocations for one listed file over an empty folder in `InMemoryVolume`, measured
/// 2026-11-18 (macOS and Linux, `cargo nextest`, debug build): the entry's parent, its
/// cloned strings, and the result `Vec`'s first allocation.
const IN_MEMORY_CEILING: u64 = 5;

#[test]
fn an_in_memory_listing_costs_at_most_its_pinned_allocations_per_entry() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    let per_entry = allocations_for_one_entry(|count| {
        let files = (0..count)
            .map(|i| FileEntry::new(format!("file_{i}.txt"), format!("/dir/file_{i}.txt"), false, false))
            .collect();
        let volume = InMemoryVolume::with_entries("Mem", files);
        let (listed, allocations) =
            allocations_during(|| runtime.block_on(volume.list_directory(Path::new("/dir"), None)));
        assert_eq!(listed.expect("lists").len() as u64, count);
        allocations
    });
    assert!(
        per_entry <= IN_MEMORY_CEILING,
        "an in-memory listing's allocations per entry went over the ceiling: {per_entry} > {IN_MEMORY_CEILING}"
    );
    assert!(per_entry > 0, "the counter is live: {per_entry}");
}
