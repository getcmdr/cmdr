//! A zip entry's time: an extra field's exact UTC second when it has one, its
//! MS-DOS field read as the writer's wall clock when it doesn't.

use std::io::{Cursor, Write};

use chrono::{FixedOffset, NaiveDateTime, TimeZone, Utc};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::*;
use crate::read::source::BytesSource;

/// The zone that stands for "local" in these tests: UTC+2, so a DOS field read
/// as UTC is two hours off.
fn utc_plus_two() -> FixedOffset {
    FixedOffset::east_opt(2 * 3600).expect("a valid offset")
}

/// 2024-03-10 12:00:00 as an MS-DOS date and time.
fn noon_dos() -> zip::DateTime {
    zip::DateTime::from_date_and_time(2024, 3, 10, 12, 0, 0).expect("a valid DOS time")
}

fn unix(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> i64 {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s)
        .single()
        .expect("a valid time")
        .timestamp()
}

/// The listed modified time of each entry, by name, read with `zone` as local.
fn modified_times<Tz: TimeZone>(bytes: Vec<u8>, zone: &Tz) -> Vec<(String, Option<i64>)> {
    let source = BytesSource::new(bytes);
    parse_in(&source, zone)
        .expect("parse")
        .into_iter()
        .map(|(raw, _)| (raw.name, raw.modified))
        .collect()
}

#[test]
fn a_dos_only_entry_reads_as_the_writers_wall_clock() {
    // What Windows Explorer, macOS Archive Utility, and most DOS-era tools write:
    // the DOS field alone, in the writer's local time.
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().last_modified_time(noon_dos());
    writer.start_file("dos-only.txt", options).expect("start");
    writer.write_all(b"x").expect("write");
    let bytes = writer.finish().expect("finish").into_inner();

    assert_eq!(
        modified_times(bytes, &utc_plus_two()),
        vec![("dos-only.txt".to_string(), Some(unix(2024, 3, 10, 10, 0, 0)))],
        "12:00 on a UTC+2 wall clock is 10:00 UTC"
    );
}

#[test]
fn an_extended_timestamp_wins_over_the_dos_field() {
    // Info-ZIP and Cmdr's own writer add the `UT` field: the exact UTC second,
    // which no zone reading changes.
    let exact = unix(2024, 3, 10, 9, 59, 31);
    let mut field = [1u8; 5];
    field[1..].copy_from_slice(&i32::try_from(exact).expect("fits").to_le_bytes());
    let mut options = SimpleFileOptions::default()
        .last_modified_time(noon_dos())
        .into_full_options();
    options.add_extra_field(0x5455, field, false).expect("add UT");

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("with-ut.txt", options).expect("start");
    writer.write_all(b"x").expect("write");
    let bytes = writer.finish().expect("finish").into_inner();

    assert_eq!(
        modified_times(bytes, &utc_plus_two()),
        vec![("with-ut.txt".to_string(), Some(exact))]
    );
}

#[test]
fn dos_only_and_extended_entries_mix_in_one_archive() {
    // The per-entry decision lines up with the right entry, in directory order.
    let exact = unix(2023, 1, 1, 0, 0, 0);
    let mut field = [1u8; 5];
    field[1..].copy_from_slice(&i32::try_from(exact).expect("fits").to_le_bytes());
    let mut with_ut = SimpleFileOptions::default()
        .last_modified_time(noon_dos())
        .into_full_options();
    with_ut.add_extra_field(0x5455, field, false).expect("add UT");
    let dos_only = SimpleFileOptions::default().last_modified_time(noon_dos());

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("a.txt", dos_only).expect("start");
    writer.write_all(b"a").expect("write");
    writer.start_file("b.txt", with_ut).expect("start");
    writer.write_all(b"b").expect("write");
    writer.add_directory("c", dos_only).expect("dir");
    let bytes = writer.finish().expect("finish").into_inner();

    let noon_in_utc_plus_two = unix(2024, 3, 10, 10, 0, 0);
    assert_eq!(
        modified_times(bytes, &utc_plus_two()),
        vec![
            ("a.txt".to_string(), Some(noon_in_utc_plus_two)),
            ("b.txt".to_string(), Some(exact)),
            ("c/".to_string(), Some(noon_in_utc_plus_two)),
        ]
    );
}

#[test]
fn a_zip64_archive_is_walked_too() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .last_modified_time(noon_dos())
        .large_file(true);
    writer.start_file("big.bin", options).expect("start");
    writer.write_all(b"x").expect("write");
    let bytes = writer.finish().expect("finish").into_inner();

    assert_eq!(
        modified_times(bytes, &utc_plus_two()),
        vec![("big.bin".to_string(), Some(unix(2024, 3, 10, 10, 0, 0)))]
    );
}

/// A zone with one summer: `standard` seconds east of UTC, `summer` from the
/// instant `starts` until `ends`. Enough DST for a gap and a fold.
#[derive(Clone, Copy, Debug)]
struct OneSummer {
    standard: i32,
    summer: i32,
    starts: i64,
    ends: i64,
}

impl OneSummer {
    /// Central European Time in 2024: UTC+1, UTC+2 from 03-31 01:00Z to 10-27 01:00Z.
    fn cet_2024() -> Self {
        Self {
            standard: 3600,
            summer: 7200,
            starts: unix(2024, 3, 31, 1, 0, 0),
            ends: unix(2024, 10, 27, 1, 0, 0),
        }
    }

    /// US Eastern in 2024: UTC-5, UTC-4 from 03-10 07:00Z to 11-03 06:00Z.
    fn us_eastern_2024() -> Self {
        Self {
            standard: -18_000,
            summer: -14_400,
            starts: unix(2024, 3, 10, 7, 0, 0),
            ends: unix(2024, 11, 3, 6, 0, 0),
        }
    }

    fn offset_at(&self, unix: i64) -> FixedOffset {
        let seconds = if (self.starts..self.ends).contains(&unix) {
            self.summer
        } else {
            self.standard
        };
        FixedOffset::east_opt(seconds).expect("a valid offset")
    }
}

impl TimeZone for OneSummer {
    type Offset = FixedOffset;

    fn from_offset(offset: &FixedOffset) -> Self {
        let seconds = offset.local_minus_utc();
        Self {
            standard: seconds,
            summer: seconds,
            starts: 0,
            ends: 0,
        }
    }

    fn offset_from_local_date(&self, local: &chrono::NaiveDate) -> chrono::LocalResult<FixedOffset> {
        self.offset_from_local_datetime(&local.and_time(chrono::NaiveTime::MIN))
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> chrono::LocalResult<FixedOffset> {
        let wall = local.and_utc().timestamp();
        let fits = |seconds: i32| {
            let offset = self.offset_at(wall - i64::from(seconds));
            (offset.local_minus_utc() == seconds).then_some(offset)
        };
        // Summer is the larger offset in both zones here, so it's the earlier instant.
        match (fits(self.summer), fits(self.standard)) {
            (Some(earlier), Some(later)) if earlier != later => chrono::LocalResult::Ambiguous(earlier, later),
            (Some(only), _) | (None, Some(only)) => chrono::LocalResult::Single(only),
            (None, None) => chrono::LocalResult::None,
        }
    }

    fn offset_from_utc_date(&self, utc: &chrono::NaiveDate) -> FixedOffset {
        self.offset_from_utc_datetime(&utc.and_time(chrono::NaiveTime::MIN))
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
        self.offset_at(utc.and_utc().timestamp())
    }
}

fn wall(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(y, mo, d)
        .and_then(|day| day.and_hms_opt(h, mi, 0))
        .expect("a valid wall clock")
}

#[test]
fn a_wall_clock_across_a_dst_change_lands_where_it_always_has() {
    let cet = OneSummer::cet_2024();
    let eastern = OneSummer::us_eastern_2024();
    let cells = [
        ("CET winter", wall(2024, 1, 15, 12, 0), cet, unix(2024, 1, 15, 11, 0, 0)),
        ("CET summer", wall(2024, 7, 1, 12, 0), cet, unix(2024, 7, 1, 10, 0, 0)),
        // The skipped hour: the offset in force at the instant the wall clock
        // reads as UTC (summer, 02:30Z is past the 01:00Z switch).
        ("CET gap", wall(2024, 3, 31, 2, 30), cet, unix(2024, 3, 31, 0, 30, 0)),
        // The repeated hour: the earlier instant, still summer.
        ("CET fold", wall(2024, 10, 27, 2, 30), cet, unix(2024, 10, 27, 0, 30, 0)),
        (
            "Eastern summer",
            wall(2024, 7, 1, 12, 0),
            eastern,
            unix(2024, 7, 1, 16, 0, 0),
        ),
        // West of UTC the same rule lands on the standard offset (02:30Z is
        // before the 07:00Z switch).
        (
            "Eastern gap",
            wall(2024, 3, 10, 2, 30),
            eastern,
            unix(2024, 3, 10, 7, 30, 0),
        ),
        (
            "Eastern fold",
            wall(2024, 11, 3, 1, 30),
            eastern,
            unix(2024, 11, 3, 5, 30, 0),
        ),
    ];
    for (name, wall, zone, expected) in cells {
        assert_eq!(zip_times::wall_clock_to_unix(wall, &zone), expected, "{name}");
    }
}

#[test]
fn a_dos_only_entry_in_the_skipped_hour_lists_at_the_same_instant() {
    let skipped = zip::DateTime::from_date_and_time(2024, 3, 31, 2, 30, 0).expect("a valid DOS time");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().last_modified_time(skipped);
    writer.start_file("spring.txt", options).expect("start");
    writer.write_all(b"x").expect("write");
    let bytes = writer.finish().expect("finish").into_inner();

    assert_eq!(
        modified_times(bytes, &OneSummer::cet_2024()),
        vec![("spring.txt".to_string(), Some(unix(2024, 3, 31, 0, 30, 0)))]
    );
}
