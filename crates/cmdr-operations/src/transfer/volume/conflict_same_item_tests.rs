//! Identity tests for `item_identity.rs::is_the_same_volume_path`. A `#[path]`
//! child of `item_identity.rs`, so `super::` is `item_identity` and
//! `super::super::` is `volume` — the same one-level-shallower rule every
//! `*_tests.rs` in this directory follows.
//!
//! The leaf is folded (the question one destination listing answers); the
//! parents are not, because whether two differently-cased directories are one is
//! the backend's call. See the function's own doc comment for what each mistake
//! costs.

use super::*;
use cmdr_fs::app_path;

#[test]
fn the_same_path_names_the_same_item() {
    assert!(is_the_same_volume_path(
        app_path!("/DCIM/photo.jpg"),
        app_path!("/DCIM/photo.jpg")
    ));
}

#[test]
fn a_case_differing_leaf_names_the_same_item() {
    assert!(is_the_same_volume_path(
        app_path!("/DCIM/Photo.JPG"),
        app_path!("/DCIM/photo.jpg")
    ));
}

#[test]
fn an_nfd_leaf_names_the_same_item_as_its_nfc_twin() {
    assert!(is_the_same_volume_path(
        app_path!("/photos/cafe\u{301}.jpg"),
        app_path!("/photos/caf\u{e9}.jpg")
    ));
}

#[test]
fn a_case_differing_parent_names_a_different_item() {
    assert!(!is_the_same_volume_path(
        app_path!("/DCIM/photo.jpg"),
        app_path!("/dcim/photo.jpg")
    ));
}

#[test]
fn a_different_parent_names_a_different_item() {
    assert!(!is_the_same_volume_path(
        app_path!("/DCIM/photo.jpg"),
        app_path!("/backup/photo.jpg")
    ));
}

/// #1491: a POSIX server's `a\b.txt` is ONE name in `/dst` on every host, so it's no
/// `b.txt` in `/dst/a` (on Windows `std::path` read both as the same item, and a move
/// to a chosen `a\b.txt` skipped as already in place). Hostile names stay names.
#[test]
fn a_backslash_name_is_one_name_in_its_folder() {
    assert!(!is_the_same_volume_path(
        app_path!("/dst/a/b.txt"),
        app_path!("/dst/a\\b.txt")
    ));
    assert!(is_the_same_volume_path(
        app_path!("/dst/a\\b.txt"),
        app_path!("/dst/A\\B.TXT")
    ));
    for (source, dest) in [
        ("/dst/foo", "/dst/foo\\.."),
        ("/bar", "/dst/..\\bar"),
        ("/dst/a/b", "/dst/a:b"),
        ("/dst/x", "/dst/x."),
        ("/dst/x", "/dst/x "),
        ("/etc/x", "/dst/C:\\x"),
    ] {
        let source = AppPath::from_normalized(source).expect("canonical");
        let dest = AppPath::from_normalized(dest).expect("canonical");
        assert!(!is_the_same_volume_path(source, dest), "{source:?} vs {dest:?}");
    }
}
