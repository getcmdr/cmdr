//! What a watcher-driven refresh does to a listing an overlay decorated.
//!
//! `.git/` is a real directory, so a pane sitting on one arms a real FSEvents
//! watch and every `.git/*` write drives a `FullRefresh` through here. The
//! refresh re-reads through the VOLUME, which holds none of the portal's six
//! category rows, so it has to run the overlays again in the same place the
//! first read did. Without that, one `git commit` under an open `.git/` pane
//! would silently empty the portal out of it.
//!
//! What the overlays themselves decide: `crates/cmdr-listing/src/host/overlays_test.rs` and
//! `file_system/git/overlay_tests.rs`. That a diff patch keeps the contributed
//! count is the engine's own cell (`crates/cmdr-listing/src/overlay_read_paths_test.rs`).

#![cfg(unix)] // POSIX-only cells: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan, task 35 and #1225).

use std::path::PathBuf;
use std::sync::Arc;

use crate::file_system::volume::manager::get_volume_manager;
use cmdr_fs::testing::unique_test_id;
use cmdr_fs::volume::Volume;
use cmdr_listing::testing::{TestListing, decorate, full_refresh};
use cmdr_local::LocalVolume;

use crate::listing_host::listings;
use cmdr_git::test_fixtures::{Fixture, cleanup, temp_dir};

const CATEGORIES: [&str; 6] = ["branches", "tags", "commits", "stash", "worktrees", "submodules"];

fn repo(name: &str) -> PathBuf {
    let dir = temp_dir("overlay_refresh", name);
    let mut fixture = Fixture::init(dir.clone());
    fixture.commit_file("README.md", b"hello\n", "initial");
    fixture.create_branch("feature/foo");
    dir
}

/// A `FullRefresh` on a repo's `.git/` keeps the six portal rows AND the flag
/// that stops a walker reusing the listing; with the portal off the same
/// refresh drops both, so the listing becomes an honest picture of the
/// directory again.
///
/// One cell rather than two: the portal switch is process-global, so a sibling
/// cell flipping it mid-run would decide this one's answer.
#[tokio::test]
async fn a_full_refresh_of_dot_git_tracks_what_the_overlay_contributes() {
    let dir = repo("full_refresh");
    let dot_git = dir.join(".git");
    crate::file_system::git::wiring::set_virtual_portal_enabled(true);

    let volume_id = unique_test_id("overlay-refresh-vol");
    let volume: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder("Repo", &dir, crate::volume_host::host()));
    get_volume_manager().register(&volume_id, Arc::clone(&volume));

    // A pane's listing as the first read left it: real entries plus the six.
    let mut entries = volume.list_directory(&dot_git, None).await.expect("listing .git");
    let added = decorate(listings(), "root", &volume, &dot_git, &mut entries).await;
    assert_eq!(added, 6);
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(dot_git.clone())
        .entries(entries)
        .overlay_rows(added)
        .insert(listings(), "overlay-refresh");

    // What the FSEvents watch on `.git/` drives after any `.git/*` write.
    refresh(&volume_id, &dot_git, listing.id()).await;

    let names = listing.entry_names();
    assert!(names.iter().any(|n| n == "HEAD"), "real entries survive: {names:?}");
    for category in CATEGORIES {
        assert!(
            names.iter().any(|n| n == category),
            "{category} lost to the refresh: {names:?}"
        );
    }
    assert!(
        listing.has_overlay_rows(),
        "the refreshed listing is still a pane view, not a walker's picture of the directory"
    );

    // Portal off: the same refresh leaves the directory and nothing else, and
    // clears the flag so a delete walker may reuse the listing again.
    crate::file_system::git::wiring::set_virtual_portal_enabled(false);
    refresh(&volume_id, &dot_git, listing.id()).await;
    crate::file_system::git::wiring::set_virtual_portal_enabled(true);

    let names = listing.entry_names();
    for category in CATEGORIES {
        assert!(!names.iter().any(|n| n == category), "{category} in {names:?}");
    }
    assert!(
        !listing.has_overlay_rows(),
        "nothing was contributed, so nothing marks this a pane view"
    );

    get_volume_manager().unregister(&volume_id);
    cleanup(&dir);
}

/// One watcher-driven `FullRefresh` of `path`, landing on `listing_id`.
async fn refresh(volume_id: &str, path: &std::path::Path, listing_id: &str) {
    full_refresh(listings(), volume_id, path, listing_id).await;
}

/// The OTHER refresh path: `refresh_listing` (⌘R, and the top-up every copy,
/// move, and delete fires when it settles) re-reads through the volume, which
/// holds none of the six category rows. It has to run the overlays for the same
/// reason the FSEvents path does, or one ⌘R on an open `.git/` pane empties the
/// portal out of it and leaves a row count claiming the six are still there.
#[tokio::test]
async fn a_listing_refresh_of_dot_git_keeps_the_portal_rows() {
    let dir = repo("listing_refresh");
    let dot_git = dir.join(".git");
    crate::file_system::git::wiring::set_virtual_portal_enabled(true);

    let volume_id = unique_test_id("overlay-listing-refresh-vol");
    let volume: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder("Repo", &dir, crate::volume_host::host()));
    get_volume_manager().register(&volume_id, Arc::clone(&volume));

    let mut entries = volume.list_directory(&dot_git, None).await.expect("listing .git");
    let added = decorate(listings(), "root", &volume, &dot_git, &mut entries).await;
    assert_eq!(added, 6);
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(dot_git.clone())
        .entries(entries)
        .overlay_rows(added)
        .insert(listings(), "overlay-listing-refresh");

    // A new real file, so the re-read differs from the cache and the diff lands.
    std::fs::write(dot_git.join("ORIG_HEAD"), b"0000\n").expect("write ORIG_HEAD");
    crate::file_system::watcher::handle_directory_change(listing.id()).await;

    let names = listing.entry_names();
    assert!(names.iter().any(|n| n == "ORIG_HEAD"), "the re-read landed: {names:?}");
    for category in CATEGORIES {
        assert!(
            names.iter().any(|n| n == category),
            "{category} lost to the refresh: {names:?}"
        );
    }
    assert!(
        listing.has_overlay_rows(),
        "the refreshed listing is still a pane view, not a walker's picture of the directory"
    );

    get_volume_manager().unregister(&volume_id);
    cleanup(&dir);
}
