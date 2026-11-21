//! `LocalWindowsVolume`'s own cells, run on real NTFS: the hostile cases the data-safety rule
//! asks of a rename that only Windows has (a name another program holds open, a read-only
//! file, a path past `MAX_PATH`), the hidden attribute, a junction, and a
//! relative bridge path spelled with `\`.
//! The cells every OS shares are in `local_volume_test.rs`.

#![cfg(target_os = "windows")] // Win32 share modes, attributes, `MAX_PATH`, and junctions

use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt as _;

use cmdr_fs::testing::AppTestDir;
use cmdr_fs::volume::{Volume, VolumeError};

use crate::local_conformance_test::junction;
use crate::test_support::{at, volume_at};

/// `FILE_SHARE_READ` alone: another program reading the file, refusing writers and deleters,
/// which is how Office and most editors hold a document open.
const FILE_SHARE_READ: u32 = 0x1;

/// A name another program holds open (no `FILE_SHARE_DELETE`): the exclusive rename refuses
/// because the name is taken, and the forced one refuses because Windows won't replace an
/// open file. Neither may touch its bytes.
#[tokio::test]
async fn a_rename_onto_a_file_another_program_holds_open_leaves_it_whole() {
    let test_dir = AppTestDir::new("win_rename_held_open");
    let volume = volume_at(&test_dir);
    volume
        .create_file(&at(&volume, "new.txt"), b"replacement")
        .await
        .expect("create");
    volume
        .create_file(&at(&volume, "doc.txt"), b"the open document")
        .await
        .expect("create");
    let _held = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(test_dir.native().join("doc.txt"))
        .expect("hold the document open");

    let exclusive = volume
        .rename(&at(&volume, "new.txt"), &at(&volume, "doc.txt"), false)
        .await
        .expect_err("the name is taken");
    assert!(matches!(exclusive, VolumeError::AlreadyExists(_)), "got {exclusive:?}");

    let forced = volume
        .rename(&at(&volume, "new.txt"), &at(&volume, "doc.txt"), true)
        .await
        .expect_err("Windows refuses to replace a file another program holds open");
    assert!(
        !matches!(forced, VolumeError::NotFound(_) | VolumeError::AlreadyExists(_)),
        "a held file is neither missing nor a name clash, got {forced:?}"
    );

    assert_eq!(
        std::fs::read(test_dir.native().join("doc.txt")).expect("read"),
        b"the open document"
    );
    assert_eq!(
        std::fs::read(test_dir.native().join("new.txt")).expect("read"),
        b"replacement"
    );
}

/// A read-only destination: the exclusive rename refuses it as taken; the forced one either
/// replaces it whole or refuses and leaves it whole, never anything in between.
#[tokio::test]
async fn a_forced_rename_onto_a_read_only_file_is_all_or_nothing() {
    let test_dir = AppTestDir::new("win_rename_read_only");
    let volume = volume_at(&test_dir);
    volume
        .create_file(&at(&volume, "new.txt"), b"replacement")
        .await
        .expect("create");
    volume
        .create_file(&at(&volume, "locked.txt"), b"read-only")
        .await
        .expect("create");
    let locked = test_dir.native().join("locked.txt");
    let mut permissions = std::fs::metadata(&locked).expect("stat").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&locked, permissions).expect("set read-only");

    let exclusive = volume
        .rename(&at(&volume, "new.txt"), &at(&volume, "locked.txt"), false)
        .await
        .expect_err("the name is taken");
    assert!(matches!(exclusive, VolumeError::AlreadyExists(_)), "got {exclusive:?}");

    match volume
        .rename(&at(&volume, "new.txt"), &at(&volume, "locked.txt"), true)
        .await
    {
        Ok(()) => {
            assert_eq!(std::fs::read(&locked).expect("read"), b"replacement");
            assert!(
                !test_dir.native().join("new.txt").exists(),
                "a replace moves the source"
            );
        }
        Err(_) => {
            assert_eq!(std::fs::read(&locked).expect("read"), b"read-only");
            assert_eq!(
                std::fs::read(test_dir.native().join("new.txt")).expect("read"),
                b"replacement"
            );
        }
    }
    let mut permissions = std::fs::metadata(&locked).expect("stat").permissions();
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "Windows has one read-only bit; clearing it lets TestDir clean up"
    )]
    permissions.set_readonly(false);
    let _ = std::fs::set_permissions(&locked, permissions);
}

/// A tree whose paths pass `MAX_PATH` (260): creating, listing, stat'ing, renaming, and
/// deleting all reach it through the `\\?\` form.
#[tokio::test]
async fn a_path_past_max_path_lists_renames_and_deletes() {
    let test_dir = AppTestDir::new("win_long_path");
    let volume = volume_at(&test_dir);
    let segment = "a_folder_name_of_fifty_characters_for_long_paths__";
    let mut deep = String::new();
    for _ in 0..6 {
        deep = if deep.is_empty() {
            segment.to_string()
        } else {
            format!("{deep}/{segment}")
        };
        volume
            .create_directory(&at(&volume, &deep))
            .await
            .expect("mkdir under a long path");
    }
    let file = format!("{deep}/file.txt");
    let full = format!("{}/{file}", volume.root().as_str());
    assert!(
        full.len() > 300,
        "the cell must pass MAX_PATH, got {} chars",
        full.len()
    );

    volume
        .create_file(&at(&volume, &file), b"deep bytes")
        .await
        .expect("create a long-path file");
    let listed = volume
        .list_directory(&at(&volume, &deep), None)
        .await
        .expect("list a long-path folder");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "file.txt");
    assert_eq!(listed[0].path, full, "a listed entry carries its app path");
    assert_eq!(listed[0].size, Some(10));

    let entry = volume
        .get_metadata(&at(&volume, &file))
        .await
        .expect("stat a long-path file");
    assert_eq!(entry.path, full);

    let renamed = format!("{deep}/renamed.txt");
    volume
        .rename(&at(&volume, &file), &at(&volume, &renamed), false)
        .await
        .expect("rename under a long path");
    volume
        .delete(&at(&volume, &renamed))
        .await
        .expect("delete a long-path file");
    assert!(
        volume
            .list_directory(&at(&volume, &deep), None)
            .await
            .expect("list")
            .is_empty()
    );
}

#[tokio::test]
async fn a_listing_reads_the_hidden_attribute() {
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    let test_dir = AppTestDir::new("win_hidden_attribute");
    let volume = volume_at(&test_dir);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .attributes(FILE_ATTRIBUTE_HIDDEN)
        .open(test_dir.native().join("secret.txt"))
        .expect("create a hidden file");
    volume
        .create_file(&at(&volume, "plain.txt"), b"x")
        .await
        .expect("create");

    let listed = volume.list_directory(&at(&volume, ""), None).await.expect("list");
    let hidden = |name: &str| listed.iter().find(|e| e.name == name).expect(name).is_hidden;
    assert!(hidden("secret.txt"));
    assert!(!hidden("plain.txt"));
}

#[tokio::test]
async fn a_folder_symlink_deletes_as_the_link_and_keeps_the_target() {
    let test_dir = AppTestDir::new("win_delete_dir_link");
    let volume = volume_at(&test_dir);
    volume.create_directory(&at(&volume, "real")).await.expect("mkdir");
    volume
        .create_file(&at(&volume, "real/keep.txt"), b"keep")
        .await
        .expect("create");
    junction(test_dir.native(), "link", "real");

    volume.delete(&at(&volume, "link")).await.expect("delete the junction");

    assert!(!test_dir.native().join("link").exists());
    assert_eq!(
        std::fs::read(test_dir.native().join("real/keep.txt")).expect("read"),
        b"keep"
    );
}

/// A relative transition-bridge path (what a caller's crate still hands in, built with `/` or
/// with `Path::join`'s `\`) lands under the root; a `..` is refused typed, ❌ never resolved by
/// text, and creates nothing. Goes with the bridge (plan milestone 11).
#[tokio::test]
async fn a_relative_bridge_path_lands_under_the_root_and_a_dot_dot_is_refused() {
    use cmdr_fs::app_path::AppPath;
    use std::path::Path;

    let test_dir = AppTestDir::new("win_relative");
    let volume = volume_at(&test_dir);
    std::fs::create_dir(test_dir.native().join("a")).expect("a");
    volume
        .create_file(&AppPath::legacy_unchecked(Path::new("a/one.txt")), b"1")
        .await
        .expect("a slashed relative path");
    volume
        .create_file(&AppPath::legacy_unchecked(Path::new(r"a\two.txt")), b"2")
        .await
        .expect("a backslashed relative path");
    assert_eq!(std::fs::read(test_dir.native().join("a/one.txt")).expect("read"), b"1");
    assert_eq!(std::fs::read(test_dir.native().join("a/two.txt")).expect("read"), b"2");

    let refused = volume
        .create_file(&AppPath::legacy_unchecked(Path::new("a/../three.txt")), b"3")
        .await;
    assert!(refused.is_err(), "{refused:?}");
    assert!(!test_dir.native().join("three.txt").exists());
}
