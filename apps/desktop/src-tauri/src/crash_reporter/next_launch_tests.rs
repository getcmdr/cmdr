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
        (at(1_777_072_708, 123_456_789), "2026-04-24T23:18:28.123456789+00:00"),
    ] {
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
