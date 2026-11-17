//! This machine's calendar, the one zone every date a person or the model reads is in.
//!
//! `cmdr-search` reads no clock and knows no zone (`cmdr_search::translate::LocalDays`);
//! this is the host's answer, through jiff's `TimeZone::system()`. The formatters here are
//! the app's only date renderings for the model: `format_local_date`
//! (`YYYY-MM-DD`, for the `*Human` and `asOf` fields) and `format_local_datetime`
//! (RFC 3339 with the local offset, for a field that shows a time of day).

use jiff::Timestamp;
use jiff::civil;
use jiff::tz::{AmbiguousOffset, TimeZone};
use time::{Date, Month};

use cmdr_search::translate::LocalDays;
use cmdr_search::{SearchQuery, query};

/// The machine's own zone, through jiff's `TimeZone::system()` (which resolves the offset
/// per instant, so a boundary or a stamp across a DST change takes that day's offset).
pub(crate) struct SystemZone;

impl LocalDays for SystemZone {
    fn today(&self) -> Date {
        from_civil(TimeZone::system().to_datetime(Timestamp::now()).date())
    }

    fn midnight(&self, date: Date) -> u64 {
        midnight_in(&TimeZone::system(), date)
    }

    fn offset_at(&self, at: u64) -> i64 {
        offset_in(&TimeZone::system(), at)
    }
}

/// The first instant of `date` in `zone`, as Unix seconds (clamped at 0).
///
/// A zone that skips midnight (a DST jump at 00:00) starts that day at the first hour that
/// exists; an ambiguous midnight starts at the earlier one. ❗ Not jiff's `.compatible()` or
/// `.earlier()`: those resolve a gap by shifting the clock, where this skips to the next hour.
fn midnight_in(zone: &TimeZone, date: Date) -> u64 {
    let day = to_civil(date);
    let start = (0..3).find_map(|hour| {
        let local = day.at(hour, 0, 0, 0);
        let offset = match zone.to_ambiguous_timestamp(local).offset() {
            AmbiguousOffset::Unambiguous { offset } => offset,
            AmbiguousOffset::Fold { before, .. } => before,
            AmbiguousOffset::Gap { .. } => return None,
        };
        // Past the edge of jiff's range (9999-12-31 east of UTC) the hour doesn't exist
        // either. The date comes from MCP or the model, so this is never an `unwrap`.
        offset.to_timestamp(local).ok()
    });
    match start {
        Some(t) => t.as_second().max(0) as u64,
        // No zone skips three hours; fall back to the date's UTC midnight.
        None => date.midnight().assume_utc().unix_timestamp().max(0) as u64,
    }
}

/// `zone`'s UTC offset at `at`, in seconds. Past jiff's last instant (9999-12-30T22:00Z),
/// the epoch's offset.
fn offset_in(zone: &TimeZone, at: u64) -> i64 {
    let at = i64::try_from(at)
        .ok()
        .and_then(|secs| Timestamp::from_second(secs).ok())
        .unwrap_or(Timestamp::UNIX_EPOCH);
    i64::from(zone.to_offset(at).seconds())
}

fn to_civil(date: Date) -> civil::Date {
    // `time` here has no `large-dates`, so its years (±9999) always fit an `i16`.
    let year = i16::try_from(date.year()).expect("a `time::Date` year fits an i16");
    civil::Date::new(year, u8::from(date.month()) as i8, date.day() as i8)
        .expect("a `time::Date` is always a valid calendar date")
}

fn from_civil(date: civil::Date) -> Date {
    let month = Month::try_from(date.month() as u8).expect("civil months are 1-12");
    Date::from_calendar_date(i32::from(date.year()), month, date.day() as u8)
        .expect("civil dates are valid calendar dates")
}

/// `YYYY-MM-DD` on this machine's calendar (`LocalDays::format_date`).
pub(crate) fn format_local_date(ts: u64) -> String {
    SystemZone.format_date(ts)
}

/// This machine's wall clock with its offset, `2026-03-29T14:00:00+02:00`
/// (`LocalDays::format_datetime`).
pub(crate) fn format_local_datetime(ts: u64) -> String {
    SystemZone.format_datetime(ts)
}

/// A query's summary, its dates on this machine's calendar (MCP's `interpretedQuery`).
pub(crate) fn summarize_query(q: &SearchQuery) -> String {
    query::summarize_query(q, &SystemZone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::fmt::temporal::DateTimePrinter;
    use time::macros::date;

    /// Central Europe: DST from the last Sunday of March 02:00 to the last Sunday of
    /// October 03:00.
    fn cet() -> TimeZone {
        TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").expect("a valid POSIX zone")
    }

    /// São Paulo's old rule: summer time starts AT midnight, so some days have no 00:00.
    fn sao_paulo_2018() -> TimeZone {
        TimeZone::posix("<-03>3<-02>,M10.3.0/0,M2.3.0/0").expect("a valid POSIX zone")
    }

    /// Like São Paulo, but summer time ends at 01:00, so 00:00–01:00 happens twice.
    fn midnight_fold() -> TimeZone {
        TimeZone::posix("<-03>3<-02>,M10.3.0/0,M2.3.0/1").expect("a valid POSIX zone")
    }

    #[test]
    fn midnight_is_the_days_first_instant_in_the_zone() {
        // DST starts at 02:00 that day, so midnight is still winter time.
        assert_eq!(midnight_in(&cet(), date!(2024 - 03 - 31)), 1_711_839_600); // 2024-03-30T23:00Z
        // DST ends at 03:00 that day, so midnight is still summer time.
        assert_eq!(midnight_in(&cet(), date!(2024 - 10 - 27)), 1_729_980_000); // 2024-10-26T22:00Z
    }

    /// 2018-10-21 has no 00:00 in São Paulo: the day starts at 01:00 (-02), never at a
    /// shifted 23:00 or 00:00.
    #[test]
    fn a_skipped_midnight_starts_the_day_at_the_next_hour() {
        assert_eq!(midnight_in(&sao_paulo_2018(), date!(2018 - 10 - 21)), 1_540_090_800); // 03:00Z
    }

    /// 00:00 happens twice on 2019-02-17: the day starts at the first one (-02).
    #[test]
    fn a_repeated_midnight_starts_the_day_at_the_earlier_one() {
        assert_eq!(midnight_in(&midnight_fold(), date!(2019 - 02 - 17)), 1_550_368_800); // 02:00Z
    }

    /// 9999-12-31 00:00 in CET is past jiff's last instant, so it lands on the date's UTC
    /// midnight, an hour later than the zone's own. A model can send this date; it must not panic.
    #[test]
    fn a_midnight_past_the_end_of_time_falls_back_to_utc() {
        assert_eq!(midnight_in(&cet(), date!(9999 - 12 - 31)), 253_402_214_400);
    }

    #[test]
    fn a_midnight_before_1970_clamps_to_the_epoch() {
        assert_eq!(midnight_in(&cet(), date!(1969 - 12 - 31)), 0);
    }

    #[test]
    fn the_offset_is_the_zones_at_that_instant() {
        assert_eq!(offset_in(&cet(), 1_705_320_000), 3_600); // 2024-01-15T12:00Z
        assert_eq!(offset_in(&cet(), 1_719_835_200), 7_200); // 2024-07-01T12:00Z
        assert_eq!(offset_in(&cet(), u64::MAX), 3_600); // out of range: the epoch's
    }

    #[test]
    fn civil_and_time_dates_convert_both_ways() {
        for d in [
            date!(2026 - 11 - 17),
            date!(2024 - 02 - 29),
            date!(-9999 - 01 - 01),
            date!(9999 - 12 - 31),
        ] {
            assert_eq!(from_civil(to_civil(d)), d);
        }
    }

    /// A day's first instant reads back as that day here, so a chip shows the date its
    /// boundary was computed from. Bites on any machine east of UTC.
    #[test]
    fn a_local_midnight_reads_back_as_its_own_date() {
        let today = SystemZone.today();
        assert_eq!(SystemZone.date_of(SystemZone.midnight(today)), today);
        let next = today.next_day().expect("not the last day");
        assert_eq!(SystemZone.date_of(SystemZone.midnight(next) - 1), today);
    }

    /// The offset is this machine's at that instant. Bites on any machine whose zone isn't
    /// UTC (the crate's fixed-zone tests pin the format and the west, east, and summer-time
    /// cases everywhere).
    #[test]
    fn a_stamp_carries_this_machines_offset_then() {
        for at in [1_768_447_800_i64, 1_784_000_000] {
            let ts = Timestamp::from_second(at).expect("in range");
            // `+00:00` for UTC too, never `Z`.
            let expected = DateTimePrinter::new()
                .precision(Some(0))
                .timestamp_with_offset_to_string(&ts, TimeZone::system().to_offset(ts));
            assert_eq!(format_local_datetime(at as u64), expected);
            assert_eq!(format_local_date(at as u64), expected[..10]);
        }
    }
}
