//! A refresh reads past the find data's lag (#1387), on real NTFS: a file another program
//! holds open and keeps appending to shows its live size after a refresh, in a small folder
//! (every entry re-read) and in one past `REFRESH_RESTAT_CAP` (the newest files re-read), and
//! the watcher's single-entry stat reads it live too. With no writer, a refresh lists exactly
//! what a listing does.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::windows::fs::OpenOptionsExt as _;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::testing::AppTestDir;
use cmdr_fs::volume::Volume;

use super::REFRESH_RESTAT_CAP;
use crate::test_support::{at, volume_at};

/// `FILE_SHARE_READ` alone, how PowerShell's `FileStream` and most loggers hold a file they
/// append to: readers welcome, no other writer.
const FILE_SHARE_READ: u32 = 0x1;

const FIRST: usize = 10_000;
const MORE: usize = 50_000;

/// Opens `grow.log` in `dir` the way a logger does, writes [`FIRST`] bytes, and hands the
/// writer back still open.
fn open_writer(dir: &AppTestDir) -> File {
    let mut writer = OpenOptions::new()
        .create_new(true)
        .append(true)
        .share_mode(FILE_SHARE_READ)
        .open(dir.native().join("grow.log"))
        .expect("open the log for appending");
    writer.write_all(&vec![b'x'; FIRST]).expect("first write");
    writer
}

fn size_of(entries: &[FileEntry], name: &str) -> Option<u64> {
    entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("{name} is listed"))
        .size
}

/// The live modified time, as the row's whole seconds.
fn live_modified(dir: &AppTestDir) -> Option<u64> {
    let modified = std::fs::metadata(dir.native().join("grow.log"))
        .expect("stat")
        .modified()
        .expect("a modified time");
    Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after 1970")
            .as_secs(),
    )
}

fn sorted_debug(mut entries: Vec<FileEntry>) -> Vec<String> {
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries.iter().map(|entry| format!("{entry:#?}")).collect()
}

/// A small folder's refresh re-reads every entry, so the open file's row is live.
#[tokio::test]
async fn a_refresh_shows_the_live_size_of_a_file_another_program_is_appending_to() {
    let dir = AppTestDir::new("win_refresh_open_writer");
    let volume = volume_at(&dir);
    std::fs::write(dir.native().join("neighbor.txt"), b"still").expect("a neighbor");
    let mut writer = open_writer(&dir);
    volume.list_directory(&at(&volume, ""), None).await.expect("list");
    writer.write_all(&vec![b'x'; MORE]).expect("append");

    let refreshed = volume
        .list_directory_for_refresh(&at(&volume, ""))
        .await
        .expect("refresh");

    assert_eq!(size_of(&refreshed, "grow.log"), Some((FIRST + MORE) as u64));
    let row = refreshed.iter().find(|entry| entry.name == "grow.log").expect("listed");
    assert_eq!(row.modified_at, live_modified(&dir), "the modified time is live too");
    assert_eq!(size_of(&refreshed, "neighbor.txt"), Some(5));
    drop(writer);
}

/// Past the cap, the refresh lists off the find data and re-reads the newest files, which
/// the file being written is one of.
#[tokio::test]
async fn a_big_folders_refresh_shows_the_live_size_of_its_newest_file() {
    let dir = AppTestDir::new("win_refresh_open_writer_big");
    let volume = volume_at(&dir);
    for i in 0..REFRESH_RESTAT_CAP + 500 {
        std::fs::write(dir.native().join(format!("old_{i:05}.txt")), b"o").expect("an older file");
    }
    let mut writer = open_writer(&dir);
    volume.list_directory(&at(&volume, ""), None).await.expect("list");
    writer.write_all(&vec![b'x'; MORE]).expect("append");

    let refreshed = volume
        .list_directory_for_refresh(&at(&volume, ""))
        .await
        .expect("refresh");

    assert_eq!(refreshed.len(), REFRESH_RESTAT_CAP + 501, "every entry, once");
    assert_eq!(size_of(&refreshed, "grow.log"), Some((FIRST + MORE) as u64));
    drop(writer);
}

/// The watcher's re-stat of a changed entry (`get_single_entry`) reads by handle, so it's
/// live while the writer still holds the file.
#[test]
fn the_watchers_single_entry_stat_reads_an_open_file_live() {
    let dir = AppTestDir::new("win_single_entry_open_writer");
    let mut writer = open_writer(&dir);
    writer.write_all(&vec![b'x'; MORE]).expect("append");

    let entry = crate::reading::get_single_entry(&dir.native().join("grow.log")).expect("stat");

    assert_eq!(entry.size, Some((FIRST + MORE) as u64));
    drop(writer);
}

/// With nobody writing, both refresh paths list exactly what a listing does, field for field.
#[tokio::test]
async fn a_refresh_with_no_writer_lists_what_a_listing_does() {
    for (label, files) in [("small", 20), ("big", REFRESH_RESTAT_CAP + 20)] {
        let dir = AppTestDir::new(&format!("win_refresh_parity_{label}"));
        let volume = volume_at(&dir);
        for i in 0..files {
            std::fs::write(dir.native().join(format!("file_{i:05}.txt")), vec![b'x'; i % 7]).expect("a file");
        }
        std::fs::create_dir(dir.native().join("sub")).expect("a folder");
        crate::local_conformance_test::junction(dir.native(), "link", "sub");

        let listed = volume.list_directory(&at(&volume, ""), None).await.expect("list");
        let refreshed = volume
            .list_directory_for_refresh(&at(&volume, ""))
            .await
            .expect("refresh");

        assert_eq!(sorted_debug(refreshed), sorted_debug(listed), "{label}");
    }
}
