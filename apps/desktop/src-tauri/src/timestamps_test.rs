//! Every expected string here was produced by chrono 0.4.45 (`to_rfc3339`,
//! `to_rfc3339_opts(SecondsFormat::Millis, true)`, and `format("%Y-%m-%dT%H:%M:%S%.3f%:z")`) for
//! the same instant, 2026-11-17. The literals are the proof that the bytes didn't move.

use jiff::Timestamp;
use jiff::tz::Offset;

use super::*;

/// 2026-11-17T12:34:56Z.
const BASE: i64 = 1_794_918_896;

/// An instant as chrono built it: whole Unix seconds plus non-negative nanos into that second.
fn at(secs: i64, nanos: u32) -> Timestamp {
    Timestamp::from_nanosecond(i128::from(secs) * 1_000_000_000 + i128::from(nanos))
        .expect("test instants are in range")
}

fn offset(seconds: i32) -> Offset {
    Offset::from_seconds(seconds).expect("test offsets are in range")
}

/// `(instant, rfc3339, rfc3339_millis_z, log stamp at +00:00, at +02:00, at -05:30)`.
fn golden() -> Vec<(Timestamp, &'static str, &'static str, [&'static str; 3])> {
    vec![
        (
            at(BASE, 0),
            "2026-11-17T12:34:56+00:00",
            "2026-11-17T12:34:56.000Z",
            [
                "2026-11-17T12:34:56.000+00:00",
                "2026-11-17T14:34:56.000+02:00",
                "2026-11-17T07:04:56.000-05:30",
            ],
        ),
        (
            at(BASE, 120_000_000),
            "2026-11-17T12:34:56.120+00:00",
            "2026-11-17T12:34:56.120Z",
            [
                "2026-11-17T12:34:56.120+00:00",
                "2026-11-17T14:34:56.120+02:00",
                "2026-11-17T07:04:56.120-05:30",
            ],
        ),
        (
            at(BASE, 123_000_000),
            "2026-11-17T12:34:56.123+00:00",
            "2026-11-17T12:34:56.123Z",
            [
                "2026-11-17T12:34:56.123+00:00",
                "2026-11-17T14:34:56.123+02:00",
                "2026-11-17T07:04:56.123-05:30",
            ],
        ),
        (
            at(BASE, 123_456_000),
            "2026-11-17T12:34:56.123456+00:00",
            "2026-11-17T12:34:56.123Z",
            [
                "2026-11-17T12:34:56.123+00:00",
                "2026-11-17T14:34:56.123+02:00",
                "2026-11-17T07:04:56.123-05:30",
            ],
        ),
        (
            at(BASE, 123_456_789),
            "2026-11-17T12:34:56.123456789+00:00",
            "2026-11-17T12:34:56.123Z",
            [
                "2026-11-17T12:34:56.123+00:00",
                "2026-11-17T14:34:56.123+02:00",
                "2026-11-17T07:04:56.123-05:30",
            ],
        ),
        (
            at(BASE, 100_000),
            "2026-11-17T12:34:56.000100+00:00",
            "2026-11-17T12:34:56.000Z",
            [
                "2026-11-17T12:34:56.000+00:00",
                "2026-11-17T14:34:56.000+02:00",
                "2026-11-17T07:04:56.000-05:30",
            ],
        ),
        (
            at(BASE, 1),
            "2026-11-17T12:34:56.000000001+00:00",
            "2026-11-17T12:34:56.000Z",
            [
                "2026-11-17T12:34:56.000+00:00",
                "2026-11-17T14:34:56.000+02:00",
                "2026-11-17T07:04:56.000-05:30",
            ],
        ),
        // Pre-1970: `Timestamp::subsec_nanosecond` is negative here, so these pin the precision
        // choice and the truncation direction.
        (
            at(-1, 999_000_000),
            "1969-12-31T23:59:59.999+00:00",
            "1969-12-31T23:59:59.999Z",
            [
                "1969-12-31T23:59:59.999+00:00",
                "1970-01-01T01:59:59.999+02:00",
                "1969-12-31T18:29:59.999-05:30",
            ],
        ),
        (
            at(-630_720_001, 123_456_789),
            "1950-01-05T23:59:59.123456789+00:00",
            "1950-01-05T23:59:59.123Z",
            [
                "1950-01-05T23:59:59.123+00:00",
                "1950-01-06T01:59:59.123+02:00",
                "1950-01-05T18:29:59.123-05:30",
            ],
        ),
    ]
}

const LOG_OFFSETS: [i32; 3] = [0, 7_200, -19_800];

#[test]
fn rfc3339_writes_chronos_bytes() {
    for (ts, expected, _, _) in golden() {
        assert_eq!(rfc3339(ts), expected, "instant {ts:?}");
    }
}

#[test]
fn rfc3339_millis_z_writes_chronos_bytes() {
    for (ts, _, expected, _) in golden() {
        assert_eq!(rfc3339_millis_z(ts), expected, "instant {ts:?}");
    }
}

#[test]
fn log_file_stamp_writes_chronos_bytes_at_every_offset() {
    for (ts, _, _, stamps) in golden() {
        for (seconds, expected) in LOG_OFFSETS.into_iter().zip(stamps) {
            let stamp = log_file_stamp(ts, offset(seconds));
            assert_eq!(stamp, expected, "instant {ts:?} at offset {seconds}");
            assert_eq!(stamp.len(), 29, "the log parser reads a 29-byte prefix");
        }
    }
}

#[test]
fn now_helpers_have_the_same_shapes() {
    let auto = now_rfc3339();
    assert!(auto.ends_with("+00:00"), "{auto}");
    assert!(parse_rfc3339(&auto).is_some(), "{auto}");

    let millis = now_rfc3339_millis_z();
    assert_eq!(millis.len(), 24, "{millis}");
    assert!(millis.ends_with('Z'), "{millis}");
    assert!(parse_rfc3339(&millis).is_some(), "{millis}");
}

#[test]
fn local_offset_now_is_a_real_zone_offset() {
    // Machine-dependent by nature; pin only what holds in every zone on Earth.
    let seconds = local_offset_now().seconds();
    assert!((-12 * 3_600..=14 * 3_600).contains(&seconds), "{seconds}");
}

/// `(input, instant)` cells chrono's `parse_from_rfc3339` produced.
#[test]
fn parse_rfc3339_accepts_what_chrono_accepted() {
    let cells = [
        ("2026-11-17T12:34:56Z", at(BASE, 0)),
        ("2026-11-17T12:34:56z", at(BASE, 0)),
        ("2026-11-17T12:34:56+00:00", at(BASE, 0)),
        ("2026-11-17T12:34:56-00:00", at(BASE, 0)),
        ("2026-11-17t12:34:56Z", at(BASE, 0)),
        ("2026-11-17 12:34:56Z", at(BASE, 0)),
        ("2026-11-17T14:34:56+02:00", at(BASE, 0)),
        ("2026-11-17T07:04:56-05:30", at(BASE, 0)),
        ("2026-11-17T12:34:56.123Z", at(BASE, 123_000_000)),
        ("2026-11-17T12:34:56.123456Z", at(BASE, 123_456_000)),
        ("2026-11-17T12:34:56.123456789+00:00", at(BASE, 123_456_789)),
        ("1950-01-01T00:00:00.5-03:00", at(-631_141_200, 500_000_000)),
    ];
    for (input, expected) in cells {
        assert_eq!(parse_rfc3339(input), Some(expected), "{input}");
    }
}

#[test]
fn parse_rfc3339_rejects_what_chrono_rejected() {
    for input in [
        "",
        "not a date",
        "2026-11-17",
        "2026-11-17T12:34:56",
        "2026-13-01T00:00:00Z",
    ] {
        assert_eq!(parse_rfc3339(input), None, "{input:?}");
    }
}

#[test]
fn parse_rfc3339_rejects_more_than_nine_fraction_digits() {
    // chrono truncated these; jiff refuses. Nothing we read writes more than nine.
    assert_eq!(parse_rfc3339("2026-11-17T12:34:56.1234567891Z"), None);
}

#[test]
fn every_writer_round_trips() {
    for (ts, _, _, _) in golden() {
        assert_eq!(parse_rfc3339(&rfc3339(ts)), Some(ts), "rfc3339 of {ts:?}");
        for seconds in LOG_OFFSETS {
            let truncated = parse_rfc3339(&log_file_stamp(ts, offset(seconds)));
            assert_eq!(
                truncated,
                parse_rfc3339(&rfc3339_millis_z(ts)),
                "log stamp of {ts:?} at {seconds}"
            );
        }
        // Truncation goes down the clock face (floor), before 1970 too.
        let millis = parse_rfc3339(&rfc3339_millis_z(ts)).expect("millis Z parses");
        let lost = millis.duration_until(ts);
        assert!(
            !lost.is_negative() && lost.as_nanos() < 1_000_000,
            "millis of {ts:?} lost {lost:?}"
        );
    }
}

/// Saved servers rank `last_connected_at` by comparing the raw STRINGS, so a fresh row must sort
/// after every older one, chrono-written rows included, even within one second and whatever
/// fraction digits each carries. A `Z` suffix would break this (`Z` sorts after `.` and `+`).
#[test]
fn rfc3339_rows_sort_by_time_as_strings() {
    let old_chrono_rows = [
        "2026-11-17T12:34:55.999999999+00:00",
        "2026-11-17T12:34:56+00:00",
        "2026-11-17T12:34:56.000000001+00:00",
        "2026-11-17T12:34:56.000100+00:00",
        "2026-11-17T12:34:56.120+00:00",
        "2026-11-17T12:34:56.123456+00:00",
    ];
    let fresh = [
        at(BASE, 123_456_789),
        at(BASE, 200_000_000),
        at(BASE + 1, 0),
        at(BASE + 1, 5_000),
    ]
    .map(rfc3339);
    let rows: Vec<&str> = old_chrono_rows
        .iter()
        .copied()
        .chain(fresh.iter().map(String::as_str))
        .collect();
    let mut sorted = rows.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, rows);
}
