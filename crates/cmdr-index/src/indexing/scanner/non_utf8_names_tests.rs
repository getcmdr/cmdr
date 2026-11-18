//! What the drive index stores TODAY for a file whose name isn't UTF-8, on
//! Linux (APFS and HFS+ refuse such a name, so macOS can't make one).

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::ffi::OsStrExt;

use super::test_fixtures::{scan_test_tempdir, setup_writer};
use super::*;
use crate::indexing::store::{IndexStore, ROOT_ID};

/// Scans a fresh tree holding `files` (raw name bytes, contents) and returns the
/// rows the index holds under its root, as `(name, logical size)`, sorted.
fn indexed_rows_after_scanning(files: &[(&[u8], &[u8])]) -> Vec<(String, Option<u64>)> {
    let scan_root = scan_test_tempdir();
    for (name, contents) in files {
        fs::write(scan_root.path().join(std::ffi::OsStr::from_bytes(name)), contents).expect("a fixture file");
    }
    let (writer, db_path, _db_dir) = setup_writer();
    let config = ScanConfig {
        root: scan_root.path().to_path_buf(),
        batch_size: 100,
        num_threads: 1,
        ..ScanConfig::default()
    };
    let (_handle, join_handle) = scan_volume(config, &writer, VolumeWork::for_test("scanner-test")).unwrap();
    join_handle.join().expect("scan thread panicked").expect("the scan");
    writer.flush_blocking().unwrap();
    writer.shutdown();
    let store = IndexStore::open(&db_path).unwrap();
    let mut rows: Vec<_> = store
        .list_children(ROOT_ID)
        .unwrap()
        .into_iter()
        .map(|row| (row.name, row.logical_size))
        .collect();
    rows.sort();
    rows
}

/// ❗ Today's wrong behavior (#1236): the index names a non-UTF-8 file by its
/// lossy spelling, so two names that differ only in their invalid bytes, and a
/// UTF-8 sibling literally named `caf\u{FFFD}.txt`, collapse into ONE row: the
/// index silently keeps one of the three files (which one follows the
/// directory's read order) and drops the other two. The lossless name gives
/// each its own spelling and its own row.
#[test]
fn the_index_stores_non_utf8_names_by_their_lossy_spelling() {
    assert_eq!(
        indexed_rows_after_scanning(&[(b"caf\xe9.txt", b"1")]),
        vec![("caf\u{FFFD}.txt".to_string(), Some(1))],
        "one non-UTF-8 file"
    );
    let rows = indexed_rows_after_scanning(&[
        (b"caf\xe9.txt", b"1"),
        (b"caf\xe8.txt", b"22"),
        ("caf\u{FFFD}.txt".as_bytes(), b"333"),
    ]);
    assert_eq!(rows.len(), 1, "three files, one row: {rows:?}");
    assert_eq!(rows[0].0, "caf\u{FFFD}.txt");
    assert!(
        matches!(rows[0].1, Some(1..=3)),
        "the row is one of the three files: {rows:?}"
    );
}
