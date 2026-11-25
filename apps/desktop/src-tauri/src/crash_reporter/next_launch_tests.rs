//! Tests for the crash-file stamp: how the crash reporter writes it, reads it back, and the
//! crash-loop check that reads it on the next launch.

use super::next_launch::{is_crash_loop, parse_timestamp, to_iso8601};
use super::now_iso8601;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn crash_loop_detection_recent_timestamp() {
    // A timestamp from "just now" should be detected as a crash loop
    let recent = to_iso8601(SystemTime::now());
    assert!(is_crash_loop(&recent));
}

#[test]
fn crash_loop_detection_old_timestamp() {
    let old = to_iso8601(SystemTime::now() - Duration::from_secs(5 * 60));
    assert!(!is_crash_loop(&old));
}

#[test]
fn crash_loop_detection_invalid_timestamp() {
    assert!(!is_crash_loop("not-a-timestamp"));
}

/// A crash within the threshold is a loop; one older than it, or one stamped in the future (a
/// clock that jumped back), isn't.
#[test]
fn crash_loop_detection_boundaries() {
    let now = SystemTime::now();
    assert!(is_crash_loop(&to_iso8601(now - Duration::from_secs(2))));
    assert!(!is_crash_loop(&to_iso8601(now - Duration::from_secs(6))));
    assert!(!is_crash_loop(&to_iso8601(now + Duration::from_secs(60))));
    assert!(
        is_crash_loop(&to_iso8601(now).replace("+00:00", "Z")),
        "a `Z` stamp reads the same"
    );
}

/// Crash-file stamps keep their bytes: UTC written `+00:00`, and 0, 3, 6, or 9 fraction digits,
/// whichever is exact. They read back to the same instant.
#[test]
fn crash_file_stamps_keep_their_bytes_and_read_back() {
    let at = |secs: u64, nanos: u32| UNIX_EPOCH + Duration::new(secs, nanos);
    for (time, expected) in [
        (at(1_777_072_708, 0), "2026-04-24T23:18:28+00:00"),
        (at(1_777_072_708, 120_000_000), "2026-04-24T23:18:28.120+00:00"),
        (at(1_777_072_708, 123_456_000), "2026-04-24T23:18:28.123456+00:00"),
        // A 100 ns multiple: the finest `SystemTime` Windows holds, so the stamp round-trips there too.
        (at(1_777_072_708, 123_456_700), "2026-04-24T23:18:28.123456700+00:00"),
    ] {
        assert_eq!(to_iso8601(time), expected);
        assert_eq!(parse_timestamp(expected), Some(time), "{expected}");
    }
    // Every nanosecond, where `SystemTime` keeps them (Windows rounds to 100 ns).
    #[cfg(unix)]
    {
        let (time, expected) = (at(1_777_072_708, 123_456_789), "2026-04-24T23:18:28.123456789+00:00");
        assert_eq!(to_iso8601(time), expected);
        assert_eq!(parse_timestamp(expected), Some(time), "{expected}");
    }
    for same in [
        "2026-03-22T10:00:00+00:00",
        "2026-03-22T10:00:00Z",
        "2026-03-22T12:00:00+02:00",
    ] {
        assert_eq!(parse_timestamp(same), Some(at(1_774_173_600, 0)), "{same}");
    }
    assert_eq!(parse_timestamp("not-a-timestamp"), None);
    assert_eq!(parse_timestamp("2026-03-22T10:00:00"), None, "no offset, no instant");
}

/// A time past year 9999 has no stamp, so it's written as now, the same fallback as an mtime
/// the filesystem won't give.
#[test]
fn a_crash_time_past_year_9999_is_stamped_now() {
    let before = SystemTime::now();
    let stamp = to_iso8601(UNIX_EPOCH + Duration::from_secs(300_000_000_000));
    let read_back = parse_timestamp(&stamp).expect("a now stamp reads back");
    assert!(read_back >= before - Duration::from_secs(1), "{stamp}");
    assert!(stamp.ends_with("+00:00"), "{stamp}");
}

/// A fresh crash stamp has the same shape as every other crash-file stamp.
#[test]
fn a_fresh_crash_stamp_is_canonical_utc() {
    let stamp = now_iso8601();
    assert!(stamp.ends_with("+00:00"), "{stamp}");
    let parsed = crate::timestamps::parse_rfc3339(&stamp).expect("parses");
    assert_eq!(crate::timestamps::rfc3339(parsed), stamp);
}

/// The raw crash files waiting in `dir`, by name.
fn raw_records_in(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list the dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("crash-report.") && name.ends_with(".raw"))
        .collect();
    names.sort();
    names
}

/// A native crash written moments ago is the app dying on start again, so the next launch asks
/// before sending, as it does for a panic. Before, the native path never checked, and a macOS crash
/// loop auto-sent a report on every relaunch.
#[test]
fn a_native_crash_just_before_this_launch_is_flagged_as_a_possible_loop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json = dir.path().join(super::CRASH_FILE_NAME);
    let raw = dir.path().join(super::RAW_CRASH_FILE_NAME);

    std::fs::write(&raw, super::platform::sample_raw_crash()).expect("write the record");
    super::next_launch::process_pending_crash(&json, &raw);
    let report = super::read_crash_report(&json).expect("converted");
    assert!(report.possible_crash_loop, "a crash written just now");

    std::fs::remove_file(&json).expect("dismiss");
    let file = std::fs::File::create(&raw).expect("create the record");
    std::io::Write::write_all(&mut &file, &super::platform::sample_raw_crash()).expect("write");
    file.set_modified(SystemTime::now() - Duration::from_secs(3600))
        .expect("age it");
    drop(file);
    super::next_launch::process_pending_crash(&json, &raw);
    let report = super::read_crash_report(&json).expect("converted");
    assert!(!report.possible_crash_loop, "a crash from an hour ago");
}

/// A native crash in a session that started with a report still pending used to be lost: the
/// next launch returned before reading the record, and the capture truncated it. It now waits
/// aside, and the first launch with a free slot offers it.
#[test]
fn a_native_crash_behind_a_pending_report_waits_for_the_next_free_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json = dir.path().join(super::CRASH_FILE_NAME);
    let raw = dir.path().join(super::RAW_CRASH_FILE_NAME);

    std::fs::write(&raw, super::platform::sample_raw_crash()).expect("first crash");
    super::next_launch::process_pending_crash(&json, &raw);
    let first = super::read_crash_report(&json).expect("the first crash is pending");

    std::fs::write(&raw, super::platform::sample_raw_crash()).expect("second crash");
    super::next_launch::process_pending_crash(&json, &raw);
    assert_eq!(
        super::read_crash_report(&json).expect("still pending").short_id,
        first.short_id,
        "the pending report stays the one on offer"
    );
    assert!(!raw.exists(), "off the name this session's capture truncates");
    let waiting = raw_records_in(dir.path());
    assert_eq!(waiting.len(), 1, "the second crash waits: {waiting:?}");

    std::fs::remove_file(&json).expect("the person dismisses the first");
    super::next_launch::process_pending_crash(&json, &raw);
    let second = super::read_crash_report(&json).expect("the second crash is offered now");
    assert_ne!(second.short_id, first.short_id);
    assert!(
        raw_records_in(dir.path()).is_empty(),
        "consumed: {:?}",
        raw_records_in(dir.path())
    );
}

/// An empty record is a clean session's leftover and goes away without making a report.
#[test]
fn an_empty_record_is_dropped_without_a_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json = dir.path().join(super::CRASH_FILE_NAME);
    let raw = dir.path().join(super::RAW_CRASH_FILE_NAME);
    std::fs::write(&raw, b"").expect("write");
    super::next_launch::process_pending_crash(&json, &raw);
    assert!(!json.exists());
    assert!(raw_records_in(dir.path()).is_empty());
}
