//! The path translation, on a volume rooted at a sub-collection.
//!
//! The rules themselves are `cmdr_fs::volume::remote_paths`' cells; these prove
//! this volume is wired to them, with its own prefix and its own root.

use std::path::Path;

use cmdr_fs::app_path;
use cmdr_fs::app_path::{AppPathBuf, PathFlavor};
use cmdr_fs::volume::{Volume, VolumeError};

use super::super::test_support::{make_test_volume, typed};
use super::super::testing::at;
use super::root_remote_path;

const ROOT: &str = "/srv/data";

/// The prefix this crate's test volume mints, spelled out so a cell reads as the
/// app would see it. `webdav://` whatever the transport is: `http` and `https` to
/// one host and port are the same server.
const PREFIX: &str = "webdav://test_user_1@127.0.0.1:1";

/// The root, and `/` (the bare alias a backend's own code once passed for "this volume's
/// root"), both name the server's root collection.
#[test]
fn every_spelling_of_the_root_is_the_root() {
    let volume = make_test_volume(ROOT);
    assert_eq!(volume.root().as_str(), format!("{PREFIX}{ROOT}"));
    assert_eq!(volume.to_remote_path(volume.root()).expect("the root"), ROOT);
    assert_eq!(volume.to_remote_path(app_path!("/")).expect("the root"), ROOT);
    assert_eq!(volume.root_remote(), ROOT);
}

/// A path built under the root and the prefixed text a pane holds land on one remote
/// path, and the app-facing one comes back out.
#[test]
fn a_prefixed_and_a_relative_path_land_on_the_same_remote_path_and_round_trip() {
    let volume = make_test_volume(ROOT);
    let relative = volume
        .to_remote_path(&at(&volume, "photos/a b.jpg"))
        .expect("under the root");
    let prefixed = volume
        .to_remote_path(typed(&format!("{PREFIX}/srv/data/photos/a b.jpg")))
        .expect("under the root");
    assert_eq!(relative, "/srv/data/photos/a b.jpg");
    assert_eq!(prefixed, relative);
    assert_eq!(
        volume
            .app_path_for(&at(&volume, "photos/a b.jpg"))
            .map(AppPathBuf::into_string),
        Some(format!("{PREFIX}/srv/data/photos/a b.jpg")),
        "what the listing-cache patcher spells is what a pane holds"
    );
    assert_eq!(
        volume.row_path_of(&relative),
        Some(format!("{PREFIX}/srv/data/photos/a b.jpg")),
        "and what a listed row carries"
    );
}

/// ❗ A bare server-absolute path is refused rather than anchored: with the
/// prefix in place the app never spells one, and accepting it is the hole
/// `root_anchored` would drive a doubled path through.
#[test]
fn a_bare_remote_path_is_refused() {
    let volume = make_test_volume(ROOT);
    for bare in [app_path!("/srv/data/photos"), app_path!("/etc/passwd")] {
        assert!(
            matches!(volume.to_remote_path(bare), Err(VolumeError::NotFound(_))),
            "a path with no prefix names nothing on this volume: {bare}"
        );
    }
}

#[test]
fn a_sibling_that_shares_a_prefix_is_refused() {
    let volume = make_test_volume(ROOT);
    assert!(matches!(
        volume.to_remote_path(typed(&format!("{PREFIX}/srv/data-1/photos"))),
        Err(VolumeError::NotFound(_))
    ));
}

/// Another account on the same server is another volume, and its paths are not
/// this one's.
#[test]
fn another_accounts_prefix_is_refused() {
    let volume = make_test_volume(ROOT);
    assert!(matches!(
        volume.to_remote_path(app_path!("webdav://test_user_2@127.0.0.1:1/srv/data/photos")),
        Err(VolumeError::NotFound(_))
    ));
}

/// A typed path can't carry a `..` at all, so none of these reaches the volume as an app
/// path; the `std::path` door the shared `mkdir -p` walk still uses resolves `..` before
/// its containment check, as it always did.
#[test]
fn a_dot_dot_escape_is_refused_however_it_is_spelled() {
    let volume = make_test_volume(ROOT);
    for escape in [
        "photos/../../etc".to_string(),
        format!("{PREFIX}/srv/data/../data-1"),
        "../data-1".to_string(),
    ] {
        assert!(
            AppPathBuf::parse(&escape, PathFlavor::Posix).is_err(),
            "{escape} is no app path"
        );
        assert!(
            volume.root.to_remote_path(Path::new(&escape)).is_none(),
            "{escape} must not resolve"
        );
    }
    assert_eq!(
        volume.root.to_remote_path(Path::new("photos/../docs")).expect("inside"),
        "/srv/data/docs"
    );
}

#[test]
fn the_root_of_a_volume_normalizes_to_one_spelling() {
    assert_eq!(root_remote_path(Path::new("/")), "/");
    assert_eq!(root_remote_path(Path::new("")), "/");
    assert_eq!(root_remote_path(Path::new(".")), "/");
    assert_eq!(root_remote_path(Path::new("Photos/")), "/Photos");
    assert_eq!(root_remote_path(Path::new("/Photos")), "/Photos");
}

/// ❗ **A name Cmdr creates goes out composed (NFC).** macOS hands out many local
/// names decomposed, and a WebDAV server on Linux stores the bytes the URL
/// spelled, where web servers, PHP, and scripts match bytes: a decomposed
/// `café.jpg` would look right and break every link to it. The engine respells a
/// new name through `Volume::spell_new_name`; this pins the answer it reads.
#[test]
fn a_new_name_goes_out_composed() {
    let volume = make_test_volume(ROOT);
    assert!(volume.composes_new_names());
    assert_eq!(volume.spell_new_name("cafe\u{301}.jpg"), "caf\u{e9}.jpg");
    assert_eq!(volume.spell_new_name("caf\u{e9}.jpg"), "caf\u{e9}.jpg");
}

/// ❗ **A path to an EXISTING entry keeps its exact bytes.** A read, a delete,
/// or an overwrite of a decomposed name the server holds must address THAT
/// name: a composed path would miss it, or hit a composed twin beside it.
#[test]
fn a_path_to_an_existing_entry_keeps_its_exact_bytes() {
    let volume = make_test_volume(ROOT);
    assert_eq!(
        volume
            .to_remote_path(&at(&volume, "fo\u{301}to\u{301}k/cafe\u{301}.jpg"))
            .expect("under the root"),
        "/srv/data/fo\u{301}to\u{301}k/cafe\u{301}.jpg"
    );
}

/// ❗ A server path is POSIX on every host: a name holding `\` or `:` (a Linux server's
/// `a\b 12:00.txt`) is ONE name in a listing's row and in a stat's answer, its path the
/// root's text plus that name. `Path::file_name` splits it on `\` on Windows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_holding_a_backslash_or_a_colon_is_one_name() {
    let server = super::super::fake_dav::FakeDav::start().await;
    let (volume, _) = server.connected().await;
    let name = "a\\b 12:00.txt";
    server.put_node(name, false);
    // `FakeDav` holds its nodes under `tree/`.
    let tree = super::super::testing::child(volume.root(), "tree");
    let expected_path = format!("{tree}/{name}");

    let listed = volume.list_directory(&tree, None).await.expect("the listing");
    let row = listed.iter().find(|entry| entry.name == name).expect("the row");
    assert_eq!(row.path, expected_path);

    let stat = volume
        .get_metadata(&super::super::testing::child(&tree, name))
        .await
        .expect("the stat");
    assert_eq!(stat.name, name);
    assert_eq!(stat.path, expected_path);
}

/// ❗ A member a server names `.` or `..` (percent-encoded, `%2E%2E`, so it survives the
/// href's own normalization) is dropped from the listing, ❌ never a row: its path would be
/// the folder itself or its PARENT, so a delete or a move of that row would act on a folder
/// the person never picked. Its siblings still list.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_named_dot_or_dot_dot_is_no_row() {
    let server = super::super::fake_dav::FakeDav::start().await;
    let (volume, _) = server.connected().await;
    server.put_node("box", true);
    server.put_node("box/.", false);
    server.put_node("box/..", false);
    server.put_node("box/kept.txt", false);
    let tree = super::super::testing::child(volume.root(), "tree");
    let folder = super::super::testing::child(&tree, "box");

    let listed = volume.list_directory(&folder, None).await.expect("the listing");

    let names: Vec<&str> = listed.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, vec!["kept.txt"]);
}
