use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::test_support::TestDir;

fn read(dir: &TestDir, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name)).ok()
}

/// One record the way `CoalescingWriter` hands it down: the bytes, then one flush.
fn record(log: &mut LiveStorage, line: &str) {
    log.write_all(line.as_bytes()).unwrap();
    log.flush().unwrap();
}

/// A keep-count cell of the test's own, so parallel tests don't share the app's.
fn cell(n: usize) -> &'static AtomicUsize {
    Box::leak(Box::new(AtomicUsize::new(n)))
}

/// Regression anchor for #1251: a non-zero cap set while the app runs only took effect after a
/// restart, because the file chain was built (or skipped) once, at startup.
#[test]
fn storage_turned_on_at_runtime_starts_writing_without_a_restart() {
    let dir = TestDir::new("live-storage-turned-on");
    let keep = cell(0);
    let mut log = LiveStorage::new(dir.join("cmdr.log"), 100, keep);

    record(&mut log, "while off\n");
    assert_eq!(read(&dir, "cmdr.log"), None, "an off cap creates no file");

    keep.store(4, Ordering::Relaxed);
    record(&mut log, "now on\n");
    assert_eq!(read(&dir, "cmdr.log").as_deref(), Some("now on\n"));
}

#[test]
fn storage_turned_off_at_runtime_stops_writing() {
    let dir = TestDir::new("live-storage-turned-off");
    let keep = cell(4);
    let mut log = LiveStorage::new(dir.join("cmdr.log"), 100, keep);
    record(&mut log, "kept\n");

    keep.store(0, Ordering::Relaxed);
    record(&mut log, "dropped\n");

    assert_eq!(read(&dir, "cmdr.log").as_deref(), Some("kept\n"));
}

#[test]
fn a_raised_cap_keeps_more_rotated_files_from_the_next_rotation_on() {
    let dir = TestDir::new("live-storage-raised");
    // Keep two files in all: the live one plus one rotated copy.
    let keep = cell(2);
    let mut log = LiveStorage::new(dir.join("cmdr.log"), 5, keep);
    record(&mut log, "aaaaa\n");
    record(&mut log, "bbbbb\n");
    record(&mut log, "ccccc\n");
    assert_eq!(read(&dir, "cmdr.log.2"), None, "the old cap keeps one rotated file");

    keep.store(3, Ordering::Relaxed);
    record(&mut log, "ddddd\n");

    assert_eq!(read(&dir, "cmdr.log.1").as_deref(), Some("ddddd\n"));
    assert_eq!(read(&dir, "cmdr.log.2").as_deref(), Some("ccccc\n"));
}
