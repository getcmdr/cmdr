//! The exact timestamp bytes Cmdr writes and reads: saved stores, crash files, the log file, and
//! what goes to the API server.
//!
//! Every format here is pinned byte for byte by literal cells in `timestamps_test.rs`, so files
//! written by older builds keep parsing and new ones look the same. jiff's own `Display` differs
//! (`Z` for UTC, minimal fraction digits), so call sites go through these helpers rather than
//! `to_string()`. The formats come from chrono 0.4, which Cmdr used before jiff.
//!
//! - `rfc3339`: UTC as `+00:00`, fraction 0, 3, 6, or 9 digits, whichever is exact.
//! - `rfc3339_millis_z`: always three fraction digits and `Z`.
//! - `log_file_stamp`: the app log's 29-byte line prefix, `YYYY-MM-DDTHH:MM:SS.mmm±HH:MM`.

use jiff::Timestamp;
use jiff::fmt::temporal::DateTimePrinter;
use jiff::tz::{Offset, TimeZone};

/// `ts` in RFC 3339 at UTC, written `+00:00`, with 0, 3, 6, or 9 fraction digits: the fewest of
/// those that are exact.
pub(crate) fn rfc3339(ts: Timestamp) -> String {
    // The civil reading's nanos, never `Timestamp::subsec_nanosecond`: that one is negative before
    // 1970, which would pick the wrong precision.
    let nanos = Offset::UTC.to_datetime(ts).subsec_nanosecond();
    let precision = if nanos == 0 {
        0
    } else if nanos % 1_000_000 == 0 {
        3
    } else if nanos % 1_000 == 0 {
        6
    } else {
        9
    };
    DateTimePrinter::new()
        .precision(Some(precision))
        .timestamp_with_offset_to_string(&ts, Offset::UTC)
}

/// The current instant as `rfc3339`.
pub(crate) fn now_rfc3339() -> String {
    rfc3339(Timestamp::now())
}

/// `ts` in RFC 3339 at UTC with exactly three fraction digits (truncated) and `Z`.
pub(crate) fn rfc3339_millis_z(ts: Timestamp) -> String {
    format!("{ts:.3}")
}

/// The current instant as `rfc3339_millis_z`.
pub(crate) fn now_rfc3339_millis_z() -> String {
    rfc3339_millis_z(Timestamp::now())
}

/// Parses an RFC 3339 instant: every shape Cmdr and its servers write (`Z`/`z`, `±HH:MM`, `t` or a
/// space as the separator, up to nine fraction digits), plus the extra ISO 8601 shapes jiff
/// accepts. A string with no offset is `None`.
pub(crate) fn parse_rfc3339(s: &str) -> Option<Timestamp> {
    s.parse().ok()
}

/// The app log file's line prefix: `ts` at `offset`, three fraction digits (truncated), offset as
/// `±HH:MM` (strftime `%Y-%m-%dT%H:%M:%S%.3f%:z`). Always 29 bytes for a four-digit year and a
/// whole-minute offset.
pub(crate) fn log_file_stamp(ts: Timestamp, offset: Offset) -> String {
    DateTimePrinter::new()
        .precision(Some(3))
        .timestamp_with_offset_to_string(&ts, offset)
}

/// This machine's UTC offset right now, in its system zone (DST included).
pub(crate) fn local_offset_now() -> Offset {
    TimeZone::system().to_offset(Timestamp::now())
}

#[cfg(test)]
#[path = "timestamps_test.rs"]
mod tests;
