//! A pane on an MTP folder is found by every spelling of that folder.
//!
//! MTP spells one folder two ways. `MtpVolume` reports its own mutations, and the
//! device event loop its refreshes, at the storage URL
//! (`mtp://{device}/{storage}/Documents`), which is also what the volume
//! switcher and go-to-path open. Its rows carry the inner path (`/Documents`),
//! which is what a pane navigates to when the user presses Enter on a folder. The
//! listing cache matched paths verbatim, so a pane entered with Enter never
//! received a Cmdr-made delete or move: the files stayed on screen and the user
//! acted on them again (field reports ERR-QW42X and ERR-46A6B, Pixel 8a, v0.44.0).
//!
//! These cells stop at the match rather than at the patch, because the match is
//! the step that failed.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::file_system::volume::manager::get_volume_manager;
use crate::listing_host::listings;
use crate::mtp::test_support::{self, device_lock};
use cmdr_fs::volume::Volume;
use cmdr_listing::testing::TestListing;

/// A pane entered with Enter hears about a change the volume reports at the
/// storage URL.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pane_entered_with_enter_is_found_at_the_url_changes_are_reported_under() {
    let _lock = device_lock().await;
    let device = test_support::connect_virtual_device().await;
    let volume = Arc::new(test_support::volume_for(&device, None).await);
    let volume_id = format!("{}:{}", device.id, device.storage_id);
    get_volume_manager().register(&volume_id, Arc::clone(&volume) as Arc<dyn Volume>);

    // What Enter on the `Documents` row navigates to: the row's own path.
    let root_rows = volume
        .list_directory(app_path!("/"), None)
        .await
        .expect("listing the storage root");
    let entered = root_rows
        .iter()
        .find(|entry| entry.name == "Documents")
        .expect("the fixture seeds Documents")
        .path
        .clone();
    assert!(
        !entered.starts_with("mtp://"),
        "the premise: an MTP row carries the inner spelling, got {entered}"
    );
    let rows = volume
        .list_directory(&AppPath::legacy_unchecked(Path::new(&entered)), None)
        .await
        .expect("listing Documents");
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(&entered)
        .entries(rows)
        .insert(listings(), "mtp-entered-with-enter");

    // Where `MtpVolume::notify_mutation` and the event loop report a change in it.
    let reported = PathBuf::from(format!("mtp://{}/{}/Documents", device.id, device.storage_id));
    let found: Vec<String> = listings()
        .listings_for_path_on_volume(&volume_id, &reported)
        .iter()
        .map(|id| id.as_str().to_string())
        .collect();
    assert_eq!(
        found,
        [listing.id()],
        "a delete or move reported at the storage URL must reach the pane that entered the folder with Enter"
    );

    drop(listing);
    get_volume_manager().unregister(&volume_id);
    test_support::teardown(device).await;
}

/// A pane opened at the storage URL answers the fresh-listing oracle when a
/// delete walker or copy scan asks with the inner path a row carries.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pane_opened_at_the_url_answers_the_oracle_at_the_inner_path() {
    let _lock = device_lock().await;
    let device = test_support::connect_virtual_device().await;
    let volume = Arc::new(test_support::volume_for(&device, None).await);
    let volume_id = format!("{}:{}", device.id, device.storage_id);
    get_volume_manager().register(&volume_id, Arc::clone(&volume) as Arc<dyn Volume>);

    let opened = format!("mtp://{}/{}/Documents", device.id, device.storage_id);
    let rows = volume
        .list_directory(&AppPath::legacy_unchecked(Path::new(&opened)), None)
        .await
        .expect("listing Documents at its URL");
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(&opened)
        .entries(rows)
        .insert(listings(), "mtp-opened-at-url");

    let answer = listings().authoritative_listing(&volume_id, Path::new("/Documents"));
    assert!(
        answer.is_some_and(|entries| entries.iter().any(|entry| entry.name == "report.txt")),
        "a walker asking at the inner path must be served the pane opened at the URL"
    );

    drop(listing);
    get_volume_manager().unregister(&volume_id);
    test_support::teardown(device).await;
}
