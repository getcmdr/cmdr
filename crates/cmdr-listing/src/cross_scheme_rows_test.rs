//! A listing whose rows sit on other schemes than the directory it lists.
//!
//! The app's Servers list is the `network` volume's `smb://`, and a one-place server's
//! row is its place (`sftp://…`, `webdav://…`, `s3://…`), which isn't under `smb://`. The
//! engine keys rows by path and patches by name, so none of that may assume a row's path
//! extends the listing's: a re-listed directory diffs by path, a row is found by its path
//! and by its name, and a change to `smb://` reaches the listing showing it.

use super::testing::{TestEngine, TestListing, TestListingGuard, pending_changes, publish_replacement};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::DirectoryChange;

const SERVERS: &str = "smb:///";

fn row(name: &str, path: &str) -> FileEntry {
    FileEntry::new(name.to_string(), path.to_string(), true, false)
}

fn servers(tag: &str, volume_id: &str, rows: Vec<FileEntry>) -> TestListingGuard {
    TestListing::new()
        .volume(volume_id)
        .path(SERVERS)
        .entries(rows)
        .insert(&TestEngine::new(), tag)
}

fn index_of_path(listing: &TestListingGuard, path: &str) -> Option<usize> {
    listing.with_listing(|cached| cached.index_of_path(path))
}

/// ❗ A row at `sftp://…` and one at `smb://<server>` sit side by side in `smb://`: each
/// is found by its own path and by its name, wherever its scheme points.
#[test]
fn rows_on_other_schemes_are_found_by_path_and_by_name() {
    let listing = servers(
        "cross-scheme-find",
        "network-find",
        vec![
            row("Testhost", "sftp://test_user_1@testhost:22"),
            row("Testnas", "smb://testnas"),
        ],
    );

    assert_eq!(index_of_path(&listing, "sftp://test_user_1@testhost:22"), Some(0));
    assert_eq!(index_of_path(&listing, "smb://testnas"), Some(1));
    assert_eq!(
        listing
            .engine()
            .inner
            .find_file_index(listing.id(), "Testhost", true)
            .unwrap(),
        Some(0)
    );
}

/// ❗ A re-list handed over whole (`Replaced`) diffs by path whatever the scheme: the
/// row that left goes, the one that came lands, the one that stayed is untouched.
#[test]
fn a_relist_with_rows_on_other_schemes_diffs_by_path() {
    let listing = servers(
        "cross-scheme-relist",
        "network-relist",
        vec![
            row("Testhost", "sftp://test_user_1@testhost:22"),
            row("Testnas", "smb://testnas"),
        ],
    );

    publish_replacement(
        listing.engine(),
        listing.id(),
        vec![
            row("Attic", "smb://attic"),
            row("Testhost", "sftp://test_user_1@testhost:22"),
            row("Webbox", "webdav://test_user_1@webbox:443"),
        ],
        0,
    );

    assert_eq!(listing.entry_names(), ["Attic", "Testhost", "Webbox"]);
    assert_eq!(index_of_path(&listing, "smb://testnas"), None);
    assert_eq!(index_of_path(&listing, "webdav://test_user_1@webbox:443"), Some(2));
    assert!(!pending_changes(listing.engine(), listing.id()).is_empty());
}

/// ❗ A backend's change to `smb://` (the hub's root, as the host seam spells it) reaches
/// the listing showing it, and only that volume's.
#[test]
fn a_change_to_the_servers_root_reaches_the_listing_showing_it() {
    let listing = servers(
        "cross-scheme-intake",
        "network-intake",
        vec![row("Testhost", "sftp://test_user_1@testhost:22")],
    );
    let other = TestListing::new()
        .volume("network-elsewhere")
        .path(SERVERS)
        .entries(vec![row("Kept", "smb://kept")])
        .insert(listing.engine(), "cross-scheme-other");
    // The hub as the host seam hands an open listing's path back, which is what a
    // backend reports a change under.
    let root = listing.engine().open_listing_paths("network-intake");
    assert_eq!(root.len(), 1);

    listing.engine().directory_changed(
        "network-intake",
        &root[0],
        DirectoryChange::Replaced(vec![
            row("Testhost", "sftp://test_user_1@testhost:22"),
            row("Testnas", "smb://testnas"),
        ]),
    );

    assert_eq!(listing.entry_names(), ["Testhost", "Testnas"]);
    assert_eq!(other.entry_names(), ["Kept"]);
}
