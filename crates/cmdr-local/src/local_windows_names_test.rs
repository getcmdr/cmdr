//! Names NTFS holds that Win32's plain paths misread, run on real NTFS on the Windows builder:
//! every one reaches its own file through the volume's `\\?\` crossing, never a neighbor
//! (`docs/specs/app-path-plan.md` § "The native boundary", each reproduced by hand in its
//! review round 3).

#![cfg(target_os = "windows")]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::Path;

use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::{Volume, VolumeError};

use crate::local_windows_conformance_test::volume_at;
use crate::test_support::at;

/// The names in `dir` as the OS lists them, sorted, each with its byte count.
fn on_disk(dir: &Path) -> Vec<(OsString, u64)> {
    let mut names: Vec<(OsString, u64)> = std::fs::read_dir(dir)
        .expect("the folder lists")
        .map(|entry| {
            let entry = entry.expect("an entry");
            (entry.file_name(), entry.metadata().expect("its stat").len())
        })
        .collect();
    names.sort();
    names
}

fn names(listed: &[(&str, u64)]) -> Vec<(OsString, u64)> {
    let mut names: Vec<(OsString, u64)> = listed.iter().map(|&(text, len)| (OsString::from(text), len)).collect();
    names.sort();
    names
}

/// ❗ `dup` and `dup.` are two files. A plain `D:\x\dup.` names `dup` (Win32 strips the dot),
/// so a delete of the row `dup.` through a plain path would remove `dup`, the other file.
#[tokio::test]
async fn a_delete_of_the_row_dup_dot_removes_dup_dot_and_leaves_dup() {
    let test_dir = TestDir::new("win_dup_dot");
    let volume = volume_at(&test_dir);
    volume.create_file(&at(&volume, "dup"), b"1").await.expect("create dup");
    volume
        .create_file(&at(&volume, "dup."), b"22")
        .await
        .expect("create dup.");
    assert_eq!(
        on_disk(&test_dir),
        names(&[("dup", 1), ("dup.", 2)]),
        "two files side by side"
    );

    let listed = volume.list_directory(volume.root(), None).await.expect("lists");
    let row = listed
        .iter()
        .find(|entry| entry.name == "dup.")
        .expect("the row `dup.`");
    assert_eq!(row.size, Some(2), "the row `dup.` is `dup.`'s own stat");
    volume
        .delete(&cmdr_fs::app_path::AppPathBuf::from_wire(&row.path).expect("a row path"))
        .await
        .expect("delete the row");

    assert_eq!(on_disk(&test_dir), names(&[("dup", 1)]), "only `dup.` went");
}

/// A device name and a trailing space are ordinary files through `\\?\`: each lists, stats,
/// renames, and deletes as itself. (Creating one is refused earlier, by the name rules, Windows
/// plan task 14; this is the file a Linux tool or WSL made.)
#[tokio::test]
async fn a_device_name_and_a_trailing_space_list_stat_rename_and_delete_as_files() {
    for name in ["con.txt", "NUL", "a "] {
        let test_dir = TestDir::new("win_reserved");
        let volume = volume_at(&test_dir);
        volume.create_file(&at(&volume, name), b"bytes").await.expect(name);
        assert_eq!(on_disk(&test_dir), names(&[(name, 5)]), "{name:?} created as itself");

        let listed = volume.list_directory(volume.root(), None).await.expect("lists");
        assert_eq!(listed.len(), 1, "{name:?}");
        assert_eq!(listed[0].name, name);
        assert_eq!(listed[0].size, Some(5), "{name:?} stats as the file");
        let stat = volume.get_metadata(&at(&volume, name)).await.expect(name);
        assert_eq!(stat.size, Some(5), "{name:?}");

        let renamed = format!("{name}-renamed");
        volume
            .rename(&at(&volume, name), &at(&volume, &renamed), false)
            .await
            .expect(name);
        assert_eq!(on_disk(&test_dir), names(&[(&renamed, 5)]), "{name:?} renamed");
        volume.delete(&at(&volume, &renamed)).await.expect(name);
        assert!(on_disk(&test_dir).is_empty(), "{name:?} deleted");
    }
}

/// ❗ A name holding `:` never reaches the OS: `C:/x/a:b` would write the alternate data
/// stream `b` INSIDE a file `a` (creating an empty `a` to hold it), which no listing shows.
#[tokio::test]
async fn a_name_holding_a_colon_is_refused_and_no_stream_appears() {
    let test_dir = TestDir::new("win_colon");
    let volume = volume_at(&test_dir);

    let refused = volume.create_file(&at(&volume, "a:b"), b"hidden").await;
    assert!(matches!(refused, Err(VolumeError::InvalidName(_))), "{refused:?}");
    let refused = volume.create_directory(&at(&volume, "a:b")).await;
    assert!(matches!(refused, Err(VolumeError::InvalidName(_))), "{refused:?}");
    assert!(on_disk(&test_dir).is_empty(), "no base file `a` holds a stream");
}

/// Two names differing only in an unpaired surrogate list as two rows, each its own file's
/// stat (the reader stats through the raw name). Their row names and paths are both
/// `a\u{FFFD}b` until escapes go live (plan milestone 10f gives each its own lossless spelling,
/// and this cell's names flip then).
#[tokio::test]
async fn names_differing_only_in_an_unpaired_surrogate_list_as_two_rows_with_their_own_stats() {
    let test_dir = TestDir::new("win_surrogates");
    std::fs::write(test_dir.join(OsString::from_wide(&[0x61, 0xD800, 0x62])), b"1").expect("a<D800>b");
    std::fs::write(test_dir.join(OsString::from_wide(&[0x61, 0xD801, 0x62])), b"22").expect("a<D801>b");
    let volume = volume_at(&test_dir);

    let listed = volume.list_directory(volume.root(), None).await.expect("lists");
    let mut sizes: Vec<Option<u64>> = listed.iter().map(|entry| entry.size).collect();
    sizes.sort();
    assert_eq!(sizes, [Some(1), Some(2)], "each row is its own file's stat");
    for entry in &listed {
        assert_eq!(entry.name, "a\u{FFFD}b", "today's lossy row");
        assert!(!entry.path.contains('\\'), "an app path: {}", entry.path);
    }
}

/// A native path (`C:\Users\…`, what a tempdir or the OS hands over) crosses up at the crate's
/// `&Path` entry points, so the reader lists it with app paths and the no-replace rename lands.
#[test]
fn the_native_entry_points_take_a_native_windows_path() {
    let test_dir = TestDir::new("win_native_entry");
    std::fs::write(test_dir.join("a.txt"), b"a").expect("a.txt");
    std::fs::write(test_dir.join("taken.txt"), b"the user's").expect("taken.txt");

    let listed =
        crate::reading::list_directory_core(&test_dir, cmdr_fs::volume::host::VolumeHost::detached().listings())
            .expect("a native folder lists");
    for entry in &listed {
        assert!(!entry.path.contains('\\'), "an app path: {}", entry.path);
        assert!(entry.path.ends_with(&format!("/{}", entry.name)), "{}", entry.path);
    }
    let single = crate::reading::get_single_entry(&test_dir.join("a.txt")).expect("stats");
    assert_eq!(single.size, Some(1));

    let taken = crate::rename_local_exclusive(&test_dir.join("a.txt"), &test_dir.join("taken.txt"));
    assert_eq!(
        taken.expect_err("a taken name").kind(),
        std::io::ErrorKind::AlreadyExists
    );
    crate::rename_local_exclusive(&test_dir.join("a.txt"), &test_dir.join("b.txt")).expect("renames");
    assert_eq!(on_disk(&test_dir), names(&[("b.txt", 1), ("taken.txt", 10)]));
}
