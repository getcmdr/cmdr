//! This OS's `LocalVolume` against the app's real host: the pane listing cache, the
//! panes' collator, and the OS watcher. The backend's own cells, which drive a detached
//! or recording host, are in `crates/cmdr-local/src/local_volume_test.rs`.

use crate::test_support::AppTestDir;
use cmdr_fs::volume::{MutationEvent, Volume, WatchCoverage};
use cmdr_local::LocalVolume;

/// This OS's local volume over `dir`, on the app's real host.
fn volume_at(dir: &AppTestDir) -> LocalVolume {
    LocalVolume::local_folder_at("Test", dir.app_path().to_owned(), crate::volume_host::host())
}

/// `listing_watch_coverage` follows the watcher lifecycle: `None` before a listing
/// opens, covered after `start_watching` succeeds, `None` again after `stop_watching`.
#[test]
fn test_listing_watch_coverage_flips_with_watcher_lifecycle() {
    use crate::file_system::watcher::{start_watching, stop_watching};
    use cmdr_listing::testing::TestListing;

    let test_dir = AppTestDir::new("listing_watch_coverage");
    let path = test_dir.native().to_path_buf();
    let app = test_dir.app_path();
    let volume = volume_at(&test_dir);

    // No listing yet, no watcher: no coverage.
    assert_eq!(
        volume.listing_watch_coverage(app),
        WatchCoverage::None,
        "expected no coverage before listing opens"
    );

    // Seed a listing in the cache (the watcher reads its path via the listing cache).
    let listing = TestListing::new()
        .path(crate::test_support::native_app_path(&path).as_str())
        .insert(crate::listing_host::listings(), "listing-is-watched");

    // Listing exists but no watcher: still `None` (the race-window contract).
    assert_eq!(
        volume.listing_watch_coverage(app),
        WatchCoverage::None,
        "expected no coverage during the listing->watcher gap"
    );

    // Start the watcher: now covered. A temp dir is local disk, so every writer
    // reaches the OS watcher (a network mount would report `ThisMachineOnly` here).
    start_watching(listing.id(), &path).expect("start_watching should succeed on a real temp dir");
    assert_eq!(
        volume.listing_watch_coverage(app),
        WatchCoverage::EveryWriter,
        "expected full coverage once the watcher is attached"
    );

    // Stop the watcher: back to `None`.
    stop_watching(listing.id());
    assert_eq!(
        volume.listing_watch_coverage(app),
        WatchCoverage::None,
        "expected no coverage after the watcher stops"
    );
}

/// A local listing comes back in the panes' name order (folders first, natural
/// numbers, case folded), ❌ never readdir's. The engine's walkers, the
/// rollback planner, and the watcher's fallback re-read take this order as is;
/// only the listing engine re-sorts.
#[tokio::test]
async fn a_local_listing_comes_back_in_the_panes_name_order() {
    let dir = AppTestDir::new("local_listing_order");
    for name in ["file10.txt", "b.txt", "File2.txt", "A.txt"] {
        std::fs::write(dir.native().join(name), b"x").expect("writing a scratch file");
    }
    std::fs::create_dir(dir.native().join("c-folder")).expect("making a folder");

    let volume = volume_at(&dir);
    let entries = volume
        .list_directory(dir.app_path(), None)
        .await
        .expect("listing the scratch dir");

    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["c-folder", "A.txt", "b.txt", "File2.txt", "file10.txt"]
    );
}

/// A local mutation patches the pane showing its folder on the spot, through the
/// app's composer (index, badges, then panes), without waiting for FSEvents. A
/// copy or rename that skipped it would leave the pane stale until the watcher
/// caught up, and forever on a watcher that missed the event.
#[tokio::test]
async fn a_local_mutation_patches_the_pane_showing_its_folder() {
    use cmdr_fs::testing::unique_test_id;
    use cmdr_listing::testing::TestListing;

    let dir = AppTestDir::new("local_notify_mutation");
    let volume_id = unique_test_id("local-notify");
    let volume = volume_at(&dir);
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(dir.app_path().as_str())
        .entries(Vec::new())
        .insert(crate::listing_host::listings(), "local-notify");

    std::fs::write(dir.native().join("landed.txt"), b"x").expect("writing the new file");
    volume
        .notify_mutation(
            &volume_id,
            dir.app_path(),
            MutationEvent::Created("landed.txt".to_string()),
        )
        .await;
    assert_eq!(listing.entry_names(), ["landed.txt"], "the create reaches the pane");

    std::fs::rename(dir.native().join("landed.txt"), dir.native().join("renamed.txt")).expect("renaming the file");
    volume
        .notify_mutation(
            &volume_id,
            dir.app_path(),
            MutationEvent::Renamed {
                from: "landed.txt".to_string(),
                to: "renamed.txt".to_string(),
            },
        )
        .await;
    assert_eq!(listing.entry_names(), ["renamed.txt"], "the rename reaches the pane");

    std::fs::remove_file(dir.native().join("renamed.txt")).expect("deleting the file");
    volume
        .notify_mutation(
            &volume_id,
            dir.app_path(),
            MutationEvent::Deleted("renamed.txt".to_string()),
        )
        .await;
    assert!(listing.entry_names().is_empty(), "the delete reaches the pane");
}
