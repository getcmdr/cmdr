//! Container clocks: a video's UTC capture time turned into the local wall-clock
//! time every other date in a name uses, and the wall clocks containers write as
//! text.

use jiff::SignedDuration;
use jiff::civil::DateTime;
use jiff::fmt::strtime::BrokenDownTime;
use jiff::tz::{Offset, TimeZone};

/// Turns a UTC instant into local wall-clock time. Injected so the readers' tests
/// don't depend on the machine's time zone; [`system_local`] in production.
pub(crate) type ToLocal<'a> = &'a dyn Fn(DateTime) -> DateTime;

/// `utc` on this machine's clock, with the offset in force at that instant
/// (summer time included). A UTC reading jiff can't place (past year 9999 in
/// the local zone) stays as it is.
pub(crate) fn system_local(utc: DateTime) -> DateTime {
    Offset::UTC
        .to_timestamp(utc)
        .map_or(utc, |instant| TimeZone::system().to_datetime(instant))
}

/// The wall clock `text` starts with, read by the strftime `format`; whatever
/// follows it (an offset, a fraction, padding) is ignored.
pub(crate) fn leading_wall_clock(text: &str, format: &str) -> Option<DateTime> {
    let (parsed, _consumed) = BrokenDownTime::parse_prefix(format, text).ok()?;
    parsed.to_datetime().ok()
}

/// Midnight UTC on `year-01-01`, a container clock's epoch.
pub(crate) fn epoch(year: i16) -> DateTime {
    DateTime::new(year, 1, 1, 0, 0, 0, 0).expect("January 1st at midnight exists in every year a container uses")
}

/// `epoch` plus `delta`, when that's a plausible capture time. A clock of zero is
/// "never set", and one before 1970 or past 2199 is a wrong epoch or garbage,
/// never a real recording: both are absent.
pub(crate) fn container_time(epoch: DateTime, delta: SignedDuration) -> Option<DateTime> {
    if delta.is_zero() {
        return None;
    }
    let time = epoch.checked_add(delta).ok()?;
    (1970..=2199).contains(&time.year()).then_some(time)
}
