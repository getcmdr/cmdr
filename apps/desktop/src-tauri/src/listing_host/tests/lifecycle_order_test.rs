//! When the app's change watch hears about a listing, relative to the cache.
//!
//! - The watch is the FIRST observer, so it disarms after the cache removal
//!   like every observer, and an observer after it hears a close with the watch
//!   gone.
//! - A listing on a watchable volume arms its watch in the TEST binary too
//!   (bookkeeping only, no OS stream: `file_system/DETAILS.md` § "Tests arm no OS
//!   stream"), since the test binary's engine (`listing_host::test_listings`) carries
//!   the watch observer. Tests lean on that
//!   (`local_posix_host_test::test_listing_watch_coverage_flips_with_watcher_lifecycle`,
//!   `archive_watch_integration_test`), so an engine that armed only in the app
//!   would leave them passing for the wrong reason.
//!
//! The order itself (open after the insert, close after the removal) is the
//! engine's, pinned in `crates/cmdr-listing/src/lifecycle_order_test.rs`. A detached
//! arm that lands after its listing ended dropping its watch is
//! `watcher_test::a_detached_arm_that_lost_the_race_leaves_no_watch_behind`.

use std::path::Path;
use std::sync::Arc;

use super::support::{Heard, disarms_of, heard, record_disarms_of, record_lifecycle_of};
use crate::file_system::volume::manager::get_volume_manager;
use crate::file_system::watcher::WATCHER_MANAGER;
use crate::listing_host::listings;
use crate::test_support::wait_until_async;
use cmdr_fs::ignore_poison::RwLockIgnorePoison;
use cmdr_fs::testing::unique_test_id;
use cmdr_listing::ListingId;
use cmdr_listing::testing::{TestListingGuard, read_to_end, request};

async fn stream(listing_id: &str, volume_id: &str, path: &Path) {
    read_to_end(listings(), &request(listing_id, volume_id, path))
        .await
        .expect("the volume lists");
}

/// Whether a watch is registered for `listing_id`.
fn is_watching(listing_id: &str) -> bool {
    WATCHER_MANAGER.read_ignore_poison().watches.contains_key(listing_id)
}

/// A real folder on a `LocalVolume`, registered under a fresh id.
fn local_volume(tag: &str) -> (crate::test_support::TestDir, String) {
    use cmdr_fs::volume::Volume;
    use cmdr_local::LocalVolume;

    let scratch = crate::test_support::TestDir::new(tag);
    std::fs::write(scratch.join("a.txt"), b"a").expect("scratch dir is writable");
    let volume_id = unique_test_id(&format!("{tag}-vol"));
    let volume: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Arming",
        scratch.to_path_buf(),
        crate::volume_host::host(),
    ));
    assert!(volume.can_watch_listings(), "the fixture must be watchable");
    get_volume_manager().register(&volume_id, volume);
    (scratch, volume_id)
}

/// Generous: the arm runs on the blocking pool, which a saturated run starves.
const ARMS_WITHIN: std::time::Duration = std::time::Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_streamed_listing_arms_its_watch_in_the_test_binary() {
    let (scratch, volume_id) = local_volume("arming-streamed");
    let listing = TestListingGuard::adopt(listings(), unique_test_id("arming-streamed"));

    stream(listing.id(), &volume_id, &scratch).await;

    wait_until_async(ARMS_WITHIN, "the streamed listing to arm its watch", || {
        is_watching(listing.id())
    })
    .await;
    drop(listing);
    get_volume_manager().unregister(&volume_id);
}

/// The watch is an observer: it disarms after the cache removal, like every
/// observer, and one registered after it hears the close with the watch gone.
/// Safe because a detached arm reconciles against cache membership, and an
/// FSEvents event landing in the gap finds no listing to patch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_watch_disarms_after_the_listing_left_the_cache() {
    let (scratch, volume_id) = local_volume("disarm-order");
    let listing = TestListingGuard::adopt(listings(), unique_test_id("disarm-order"));
    stream(listing.id(), &volume_id, &scratch).await;
    wait_until_async(ARMS_WITHIN, "the streamed listing to arm its watch", || {
        is_watching(listing.id())
    })
    .await;
    record_lifecycle_of(listing.id());
    record_disarms_of(listing.id());

    listings().end(&ListingId::new(listing.id()));

    assert_eq!(
        disarms_of(listing.id()),
        [false],
        "the disarm saw the listing gone from the cache"
    );
    assert_eq!(
        heard(listing.id()),
        [Heard::Closed {
            cached: false,
            watching: false
        }]
    );
    drop(listing);
    get_volume_manager().unregister(&volume_id);
}
