//! The mtime format, against the examples in `ncw/swift`'s `meta.go`.

use std::time::{Duration, UNIX_EPOCH};

use super::{format_mtime, parse_mtime};

#[test]
fn whole_seconds_have_no_decimal_point() {
    assert_eq!(
        format_mtime(UNIX_EPOCH + Duration::from_secs(1_354_040_105)),
        "1354040105"
    );
}

#[test]
fn nanoseconds_survive_and_trailing_zeros_drop() {
    // Windows' `SystemTime` counts 100 ns ticks, so it holds seven fraction digits.
    assert_eq!(
        format_mtime(UNIX_EPOCH + Duration::new(1_354_040_105, 123_456_700)),
        "1354040105.1234567"
    );
    #[cfg(unix)]
    assert_eq!(
        format_mtime(UNIX_EPOCH + Duration::new(1_354_040_105, 123_456_789)),
        "1354040105.123456789"
    );
    assert_eq!(
        format_mtime(UNIX_EPOCH + Duration::new(1_354_040_105, 500_000_000)),
        "1354040105.5"
    );
}

#[test]
fn under_a_second_keeps_a_leading_zero() {
    assert_eq!(format_mtime(UNIX_EPOCH + Duration::from_millis(5)), "0.005");
    assert_eq!(format_mtime(UNIX_EPOCH), "0");
}

#[test]
fn before_the_epoch_is_negative() {
    assert_eq!(format_mtime(UNIX_EPOCH - Duration::from_millis(1_500)), "-1.5");
    assert_eq!(parse_mtime("-1.5"), Some(UNIX_EPOCH - Duration::from_millis(1_500)));
}

#[test]
fn parsing_round_trips_and_pads_or_cuts_the_fraction_to_nanoseconds() {
    let at = UNIX_EPOCH + Duration::new(1_354_040_105, 123_456_789);
    assert_eq!(parse_mtime(&format_mtime(at)), Some(at));
    assert_eq!(
        parse_mtime("1354040105"),
        Some(UNIX_EPOCH + Duration::from_secs(1_354_040_105))
    );
    assert_eq!(parse_mtime("1.5"), Some(UNIX_EPOCH + Duration::from_millis(1_500)));
    assert_eq!(
        parse_mtime("1.1234567899"),
        Some(UNIX_EPOCH + Duration::new(1, 123_456_789))
    );
}

#[test]
fn garbage_is_not_an_mtime() {
    for text in ["", "abc", "1.2.3", "1e9", " 12", "1.x", "-"] {
        assert_eq!(parse_mtime(text), None, "{text:?}");
    }
}

// ── rclone's `--metadata` keys ───────────────────────────────────────

#[test]
fn a_creation_date_goes_out_as_rclones_rfc3339_in_utc() {
    // `time.RFC3339Nano`: the fraction only when there is one.
    assert_eq!(
        super::format_btime(UNIX_EPOCH + Duration::from_secs(1_425_168_000)).as_deref(),
        Some("2015-03-01T00:00:00Z")
    );
    assert_eq!(
        super::format_btime(UNIX_EPOCH + Duration::new(1_425_168_000, 120_000_000)).as_deref(),
        Some("2015-03-01T00:00:00.12Z")
    );
}

#[test]
fn a_creation_date_reads_back_whatever_offset_rclone_wrote() {
    // rclone's local backend formats in the machine's zone, so an offset is
    // what a bucket filled by rclone on a laptop in Stockholm holds.
    let born = UNIX_EPOCH + Duration::from_secs(1_425_168_000);
    assert_eq!(super::parse_btime("2015-03-01T01:00:00+01:00"), Some(born));
    assert_eq!(
        super::parse_btime(&super::format_btime(born).expect("a 2015 date")),
        Some(born)
    );
    assert_eq!(
        super::parse_btime("1425168000"),
        None,
        "an mtime-shaped value isn't a btime"
    );
}

#[test]
fn a_mode_goes_out_as_rclones_whole_st_mode_in_octal() {
    assert_eq!(super::format_mode(0o644), "100644");
    assert_eq!(super::format_mode(0o4755), "104755");
}

#[test]
fn a_mode_reads_back_its_permission_bits_only() {
    assert_eq!(super::parse_mode("100644"), Some(0o644));
    assert_eq!(super::parse_mode("0100664"), Some(0o664), "rclone's documented example");
    assert_eq!(
        super::parse_mode("40755"),
        Some(0o755),
        "a folder's mode reads the same way"
    );
    assert_eq!(super::parse_mode("rwxr-xr-x"), None);
    assert_eq!(super::parse_mode(""), None);
}
