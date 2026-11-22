//! The app's half of the orphan reaper: a listing the engine ends takes its
//! FSEvents watch with it. The reaper ends a stale listing the way a pane's
//! close does (`Listings::end`), which `crates/cmdr-listing/src/caching_reaper_test.rs`
//! pins against a recording observer; this pins that the app's change watch is
//! among the observers that let go.

use crate::file_system::watcher::{WATCHER_MANAGER, start_watching};
use crate::listing_host::listings;
use cmdr_fs::ignore_poison::RwLockIgnorePoison;
use cmdr_listing::ListingId;
use cmdr_listing::testing::TestListing;

fn is_watched(id: &str) -> bool {
    WATCHER_MANAGER.read_ignore_poison().watches.contains_key(id)
}

#[test]
fn reaper_evicts_stale_listing_and_its_watcher_together() {
    // A real directory, since `start_watching` refuses a path the OS would.
    let dir = tempfile::tempdir().unwrap();
    let listing = TestListing::new()
        .path(crate::test_support::native_app_path(dir.path()).as_str())
        .last_accessed_ms(0)
        .insert(listings(), "orphan");
    start_watching(listing.id(), dir.path()).expect("start_watching should succeed on a real dir");
    assert!(is_watched(listing.id()), "precondition: watcher is attached");

    listings().end(&ListingId::new(listing.id()));

    assert!(!listing.is_cached(), "the end removes the cache entry");
    assert!(
        !is_watched(listing.id()),
        "the end tears the watcher down too, through the watch observer"
    );
}
