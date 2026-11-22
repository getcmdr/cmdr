//! `LocalWindowsVolume`'s own cells, run on real NTFS on the Windows builder: the hostile
//! cases the data-safety rule asks of a rename (a taken name, a name another program holds
//! open, a read-only file, a path past `MAX_PATH`), the long-path reader, and space.

#![cfg(target_os = "windows")]

use crate::test_support::at;
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt as _;

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
    volume
        .create_file(&at(&volume, "a.txt"), b"source")
        .await
        .expect("create");
    volume
        .create_file(&at(&volume, "b.txt"), b"the user's file")
        .await
        .expect("create");

    let err = volume
        .rename(&at(&volume, "a.txt"), &at(&volume, "b.txt"), false)
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
    volume.create_directory(&at(&volume, "from")).await.expect("mkdir");
    volume.create_directory(&at(&volume, "to")).await.expect("mkdir");
    volume
        .create_file(&at(&volume, "to/keep.txt"), b"keep")
        .await
        .expect("create");

    let err = volume
        .rename(&at(&volume, "from"), &at(&volume, "to"), false)
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
        .open(test_dir.join("doc.txt"))
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
        .create_file(&at(&volume, "new.txt"), b"replacement")
        .await
        .expect("create");
    volume
        .create_file(&at(&volume, "locked.txt"), b"read-only")
        .await
        .expect("create");
    let locked = test_dir.join("locked.txt");
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

fn set_read_only(path: &std::path::Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).expect("stat").permissions();
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "Windows has one read-only bit; clearing it lets TestDir clean up"
    )]
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).expect("set the attribute");
}

/// A read-only file is Windows' Locked: `delete` refuses it the way macOS's `unlink` refuses
/// a Locked file, where `std::fs::remove_file` alone would delete it. `clear_blockers` (a
/// consented move's unlock, a reversal's retry) clears it, and the retry deletes. A folder's
/// read-only bit is Explorer's customization marker, so there's nothing to clear on one.
#[tokio::test]
async fn a_delete_refuses_a_read_only_file_until_its_blockers_are_cleared() {
    let test_dir = TestDir::new("win_delete_read_only");
    let volume = volume_at(&test_dir);
    volume
        .create_file(&at(&volume, "frozen.txt"), b"keep me")
        .await
        .expect("create");
    volume
        .create_directory(&at(&volume, "customized"))
        .await
        .expect("mkdir");
    let frozen = test_dir.join("frozen.txt");
    set_read_only(&frozen, true);
    set_read_only(&test_dir.join("customized"), true);

    let refused = volume
        .delete(&at(&volume, "frozen.txt"))
        .await
        .expect_err("a read-only file refuses its delete");
    assert!(
        matches!(refused, VolumeError::PermissionDenied { .. }),
        "got {refused:?}"
    );
    assert_eq!(std::fs::read(&frozen).expect("read"), b"keep me");

    assert!(matches!(
        volume.clear_blockers(&at(&volume, "frozen.txt")).await,
        cmdr_fs::metadata::Unblock::Cleared(_)
    ));
    volume
        .delete(&at(&volume, "frozen.txt"))
        .await
        .expect("cleared, it goes");
    assert!(!frozen.exists());
    assert_eq!(
        volume.clear_blockers(&at(&volume, "customized")).await,
        cmdr_fs::metadata::Unblock::NothingToClear,
        "a folder's marker isn't a lock"
    );
    set_read_only(&test_dir.join("customized"), false);
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
            .create_directory(&at(&volume, &deep))
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
    let test_dir = TestDir::new("win_hidden_attribute");
    let volume = volume_at(&test_dir);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .attributes(FILE_ATTRIBUTE_HIDDEN)
        .open(test_dir.join("secret.txt"))
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
    let test_dir = TestDir::new("win_delete_dir_link");
    let volume = volume_at(&test_dir);
    volume.create_directory(&at(&volume, "real")).await.expect("mkdir");
    volume
        .create_file(&at(&volume, "real/keep.txt"), b"keep")
        .await
        .expect("create");
    junction(&test_dir, "link", "real");

    volume.delete(&at(&volume, "link")).await.expect("delete the junction");

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

/// A relative transition-bridge path (what a caller's crate still hands in, built with `/` or
/// with `Path::join`'s `\`) lands under the root; a `..` is refused typed, ❌ never resolved by
/// text, and creates nothing. Goes with the bridge (plan milestone 11).
#[tokio::test]
async fn a_relative_bridge_path_lands_under_the_root_and_a_dot_dot_is_refused() {
    use cmdr_fs::app_path::AppPath;
    use std::path::Path;

    let test_dir = TestDir::new("win_relative");
    let volume = volume_at(&test_dir);
    std::fs::create_dir(test_dir.join("a")).expect("a");
    volume
        .create_file(&AppPath::legacy_unchecked(Path::new("a/one.txt")), b"1")
        .await
        .expect("a slashed relative path");
    volume
        .create_file(&AppPath::legacy_unchecked(Path::new(r"a\two.txt")), b"2")
        .await
        .expect("a backslashed relative path");
    assert_eq!(std::fs::read(test_dir.join("a/one.txt")).expect("read"), b"1");
    assert_eq!(std::fs::read(test_dir.join("a/two.txt")).expect("read"), b"2");

    let refused = volume
        .create_file(&AppPath::legacy_unchecked(Path::new("a/../three.txt")), b"3")
        .await;
    assert!(refused.is_err(), "{refused:?}");
    assert!(!test_dir.join("three.txt").exists());
}

/// A case-only rename's old spelling reads as gone to the watcher's stat, though NTFS's own stat
/// still answers for it, and the new one as the entry under its own name.
#[test]
fn a_case_only_rename_leaves_the_old_spelling_gone_for_the_watcher() {
    use crate::reading::get_single_entry;

    let test_dir = TestDir::new("win_case_rename");
    let root = app_root(&test_dir);
    std::fs::write(test_dir.join("a.txt"), b"x").expect("write");
    std::fs::rename(test_dir.join("a.txt"), test_dir.join("A.txt")).expect("case-only rename");

    let root = root.to_str().expect("unicode");
    let old = std::path::PathBuf::from(format!("{root}/a.txt"));
    let new = std::path::PathBuf::from(format!("{root}/A.txt"));
    assert!(std::fs::metadata(&old).is_ok(), "NTFS answers case-insensitively");
    let err = get_single_entry(&old).expect_err("the old spelling isn't on disk");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    let entry = get_single_entry(&new).expect("the new spelling is");
    assert_eq!(entry.name, "A.txt");
    assert_eq!(
        get_single_entry(&test_dir.join("gone.txt"))
            .expect_err("nothing there")
            .kind(),
        std::io::ErrorKind::NotFound
    );
}

/// A watch opens the folder's verbatim form, so a path past `MAX_PATH` arms, and its event
/// paths come back in the app spelling the listing and its rows carry.
#[test]
fn a_watch_opens_the_verbatim_path_and_its_events_read_back_in_the_app_spelling() {
    use crate::reading::{listing_spelling_of, watch_os_path};
    use std::path::{Path, PathBuf};

    assert_eq!(
        watch_os_path(Path::new("C:/Users/test_user_1")).expect("an app path"),
        PathBuf::from(r"\\?\C:\Users\test_user_1")
    );
    assert_eq!(
        listing_spelling_of(Path::new(r"\\?\C:\Users\test_user_1\a.txt")),
        PathBuf::from("C:/Users/test_user_1/a.txt")
    );
    assert_eq!(
        listing_spelling_of(Path::new(r"\\?\UNC\testnas.local\share\x\a.txt")),
        PathBuf::from("//testnas.local/share/x/a.txt")
    );

    let test_dir = TestDir::new("win_watch_long");
    let mut long = app_root(&test_dir).to_str().expect("unicode").to_owned();
    while long.len() < 300 {
        long.push_str("/a-folder-name-of-forty-characters-123456");
    }
    let os = watch_os_path(Path::new(&long)).expect("a long app path");
    std::fs::create_dir_all(&os).expect("the verbatim form makes a long path");
    assert!(os.is_dir());
    assert_eq!(
        listing_spelling_of(&os.join("x.txt")),
        PathBuf::from(format!("{long}/x.txt"))
    );
}

/// A local drive carries a listing watch (#1330), and its coverage is the host's answer for the
/// folder in the app spelling the listing cache keys on (`C:/…`, never `\\?\C:\…`): the
/// watcher decided it at arm time from the drive kind.
#[test]
fn a_local_drive_watches_its_listings_and_asks_the_host_in_the_app_spelling() {
    use cmdr_fs::volume::WatchCoverage;
    use cmdr_fs::volume::host::VolumeHost;
    use cmdr_fs::volume::host::listings::RecordingListings;
    use std::sync::Arc;

    let test_dir = TestDir::new("win_watch_coverage");
    let shown = app_root(&test_dir.join("shown"));
    let listings = RecordingListings::new().with_local_watch_coverage(shown, WatchCoverage::EveryWriter);
    let host = VolumeHost::builder().listings(Arc::new(listings)).build();
    let root = cmdr_fs::app_path::AppPathBuf::from_native_unicode(&test_dir).expect("the test folder is an app path");
    let volume = crate::LocalWindowsVolume::local_folder_at("Test", root, host);

    assert!(volume.can_watch_listings());
    assert_eq!(
        volume.listing_watch_coverage(&at(&volume, "shown")),
        WatchCoverage::EveryWriter
    );
    assert_eq!(
        volume.listing_watch_coverage(&at(&volume, "elsewhere")),
        WatchCoverage::None
    );
}
