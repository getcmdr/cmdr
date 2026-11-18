//! `LocalWindowsVolume`'s own cells, run on real NTFS on the Windows builder: the hostile
//! cases the data-safety rule asks of a rename (a taken name, a name another program holds
//! open, a read-only file, a path past `MAX_PATH`), the long-path reader, and space.

#![cfg(target_os = "windows")]

use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::Path;

use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::{SpaceInfo, Volume, VolumeError};

use crate::local_windows_conformance_test::{app_root, junction, volume_at};

/// `FILE_SHARE_READ` alone: another program reading the file, refusing writers and deleters,
/// which is how Office and most editors hold a document open.
const FILE_SHARE_READ: u32 = 0x1;

#[tokio::test]
async fn an_exclusive_rename_onto_a_taken_name_refuses_and_keeps_both_files() {
    let test_dir = TestDir::new("win_rename_taken");
    let volume = volume_at(&test_dir);
    volume.create_file(Path::new("a.txt"), b"source").await.expect("create");
    volume
        .create_file(Path::new("b.txt"), b"the user's file")
        .await
        .expect("create");

    let err = volume
        .rename(Path::new("a.txt"), Path::new("b.txt"), false)
        .await
        .expect_err("an exclusive rename onto a taken name must refuse");

    assert!(matches!(err, VolumeError::AlreadyExists(_)), "got {err:?}");
    assert_eq!(std::fs::read(test_dir.join("b.txt")).expect("read"), b"the user's file");
    assert_eq!(std::fs::read(test_dir.join("a.txt")).expect("read"), b"source");
}

#[tokio::test]
async fn an_exclusive_rename_of_a_folder_onto_a_taken_name_refuses() {
    let test_dir = TestDir::new("win_rename_dir_taken");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("from")).await.expect("mkdir");
    volume.create_directory(Path::new("to")).await.expect("mkdir");
    volume
        .create_file(Path::new("to/keep.txt"), b"keep")
        .await
        .expect("create");

    let err = volume
        .rename(Path::new("from"), Path::new("to"), false)
        .await
        .expect_err("a folder onto a taken name must refuse");

    assert!(matches!(err, VolumeError::AlreadyExists(_)), "got {err:?}");
    assert_eq!(std::fs::read(test_dir.join("to/keep.txt")).expect("read"), b"keep");
}

/// A name another program holds open (no `FILE_SHARE_DELETE`): the exclusive rename refuses
/// because the name is taken, and the forced one refuses because Windows won't replace an
/// open file. Neither may touch its bytes.
#[tokio::test]
async fn a_rename_onto_a_file_another_program_holds_open_leaves_it_whole() {
    let test_dir = TestDir::new("win_rename_held_open");
    let volume = volume_at(&test_dir);
    volume
        .create_file(Path::new("new.txt"), b"replacement")
        .await
        .expect("create");
    volume
        .create_file(Path::new("doc.txt"), b"the open document")
        .await
        .expect("create");
    let _held = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(test_dir.join("doc.txt"))
        .expect("hold the document open");

    let exclusive = volume
        .rename(Path::new("new.txt"), Path::new("doc.txt"), false)
        .await
        .expect_err("the name is taken");
    assert!(matches!(exclusive, VolumeError::AlreadyExists(_)), "got {exclusive:?}");

    let forced = volume
        .rename(Path::new("new.txt"), Path::new("doc.txt"), true)
        .await
        .expect_err("Windows refuses to replace a file another program holds open");
    assert!(
        !matches!(forced, VolumeError::NotFound(_) | VolumeError::AlreadyExists(_)),
        "a held file is neither missing nor a name clash, got {forced:?}"
    );

    assert_eq!(
        std::fs::read(test_dir.join("doc.txt")).expect("read"),
        b"the open document"
    );
    assert_eq!(std::fs::read(test_dir.join("new.txt")).expect("read"), b"replacement");
}

/// A read-only destination: the exclusive rename refuses it as taken; the forced one either
/// replaces it whole or refuses and leaves it whole, never anything in between.
#[tokio::test]
async fn a_forced_rename_onto_a_read_only_file_is_all_or_nothing() {
    let test_dir = TestDir::new("win_rename_read_only");
    let volume = volume_at(&test_dir);
    volume
        .create_file(Path::new("new.txt"), b"replacement")
        .await
        .expect("create");
    volume
        .create_file(Path::new("locked.txt"), b"read-only")
        .await
        .expect("create");
    let locked = test_dir.join("locked.txt");
    let mut permissions = std::fs::metadata(&locked).expect("stat").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&locked, permissions).expect("set read-only");

    let exclusive = volume
        .rename(Path::new("new.txt"), Path::new("locked.txt"), false)
        .await
        .expect_err("the name is taken");
    assert!(matches!(exclusive, VolumeError::AlreadyExists(_)), "got {exclusive:?}");

    match volume.rename(Path::new("new.txt"), Path::new("locked.txt"), true).await {
        Ok(()) => {
            assert_eq!(std::fs::read(&locked).expect("read"), b"replacement");
            assert!(!test_dir.join("new.txt").exists(), "a replace moves the source");
        }
        Err(_) => {
            assert_eq!(std::fs::read(&locked).expect("read"), b"read-only");
            assert_eq!(std::fs::read(test_dir.join("new.txt")).expect("read"), b"replacement");
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
    let test_dir = TestDir::new("win_long_path");
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
            .create_directory(Path::new(&deep))
            .await
            .expect("mkdir under a long path");
    }
    let file = format!("{deep}/file.txt");
    let full = format!("{}/{file}", app_root(&test_dir).display());
    assert!(
        full.len() > 300,
        "the cell must pass MAX_PATH, got {} chars",
        full.len()
    );

    volume
        .create_file(Path::new(&file), b"deep bytes")
        .await
        .expect("create a long-path file");
    let listed = volume
        .list_directory(Path::new(&deep), None)
        .await
        .expect("list a long-path folder");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "file.txt");
    assert_eq!(listed[0].path, full, "a listed entry carries its app path");
    assert_eq!(listed[0].size, Some(10));

    let entry = volume
        .get_metadata(Path::new(&file))
        .await
        .expect("stat a long-path file");
    assert_eq!(entry.path, full);

    let renamed = format!("{deep}/renamed.txt");
    volume
        .rename(Path::new(&file), Path::new(&renamed), false)
        .await
        .expect("rename under a long path");
    volume
        .delete(Path::new(&renamed))
        .await
        .expect("delete a long-path file");
    assert!(
        volume
            .list_directory(Path::new(&deep), None)
            .await
            .expect("list")
            .is_empty()
    );
}

#[tokio::test]
async fn a_listing_reads_the_hidden_attribute() {
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    let test_dir = TestDir::new("win_hidden_attribute");
    let volume = volume_at(&test_dir);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .attributes(FILE_ATTRIBUTE_HIDDEN)
        .open(test_dir.join("secret.txt"))
        .expect("create a hidden file");
    volume.create_file(Path::new("plain.txt"), b"x").await.expect("create");

    let listed = volume.list_directory(Path::new(""), None).await.expect("list");
    let hidden = |name: &str| listed.iter().find(|e| e.name == name).expect(name).is_hidden;
    assert!(hidden("secret.txt"));
    assert!(!hidden("plain.txt"));
}

#[tokio::test]
async fn a_folder_symlink_deletes_as_the_link_and_keeps_the_target() {
    let test_dir = TestDir::new("win_delete_dir_link");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("real")).await.expect("mkdir");
    volume
        .create_file(Path::new("real/keep.txt"), b"keep")
        .await
        .expect("create");
    junction(&test_dir, "link", "real");

    volume.delete(Path::new("link")).await.expect("delete the junction");

    assert!(!test_dir.join("link").exists());
    assert_eq!(std::fs::read(test_dir.join("real/keep.txt")).expect("read"), b"keep");
}

#[tokio::test]
async fn the_root_reports_a_bounded_capacity() {
    let test_dir = TestDir::new("win_space");
    let volume = volume_at(&test_dir);

    let space = volume.get_space_info().await.expect("space");

    let SpaceInfo::Bounded {
        total_bytes,
        available_bytes,
        used_bytes,
    } = space
    else {
        panic!("a mounted filesystem always has a capacity, got {space:?}");
    };
    assert!(total_bytes > 0);
    assert!(available_bytes <= total_bytes);
    assert!(used_bytes <= total_bytes);
}
