//! A zip entry's time: an extra field's exact UTC second when it has one, its
//! MS-DOS field read as the writer's wall clock when it doesn't.

use std::io::{Cursor, Write};

use jiff::civil::DateTime;
use jiff::tz::Offset;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::*;
use crate::read::source::BytesSource;

/// The zone that stands for "local" in these tests: UTC+2, so a DOS field read
/// as UTC is two hours off.
fn utc_plus_two() -> TimeZone {
    TimeZone::fixed(Offset::from_seconds(2 * 3600).expect("a valid offset"))
}

/// Central European Time: UTC+1, UTC+2 from the last Sunday of March to the last of October.
fn cet() -> TimeZone {
    TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").expect("a valid POSIX zone")
}

/// US Eastern: UTC-5, UTC-4 from the second Sunday of March to the first of November.
fn us_eastern() -> TimeZone {
    TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").expect("a valid POSIX zone")
}

/// 2024-03-10 12:00:00 as an MS-DOS date and time.
fn noon_dos() -> zip::DateTime {
    zip::DateTime::from_date_and_time(2024, 3, 10, 12, 0, 0).expect("a valid DOS time")
}

fn wall(y: i16, mo: i8, d: i8, h: i8, mi: i8) -> DateTime {
    DateTime::new(y, mo, d, h, mi, 0, 0).expect("a valid wall clock")
}

fn unix(y: i16, mo: i8, d: i8, h: i8, mi: i8, s: i8) -> i64 {
    let utc = DateTime::new(y, mo, d, h, mi, s, 0).expect("a valid time");
    Offset::UTC.to_timestamp(utc).expect("in range").as_second()
}

/// The listed modified time of each entry, by name, read with `zone` as local.
fn modified_times(bytes: Vec<u8>, zone: &TimeZone) -> Vec<(String, Option<i64>)> {
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

#[test]
fn a_wall_clock_across_a_dst_change_lands_where_it_always_has() {
    let (cet, eastern) = (cet(), us_eastern());
    let cells = [
        (
            "CET winter",
            wall(2024, 1, 15, 12, 0),
            &cet,
            unix(2024, 1, 15, 11, 0, 0),
        ),
        ("CET summer", wall(2024, 7, 1, 12, 0), &cet, unix(2024, 7, 1, 10, 0, 0)),
        // The skipped hour: the offset in force at the instant the wall clock
        // reads as UTC (summer, 02:30Z is past the 01:00Z switch).
        ("CET gap", wall(2024, 3, 31, 2, 30), &cet, unix(2024, 3, 31, 0, 30, 0)),
        // The repeated hour: the earlier instant, still summer.
        (
            "CET fold",
            wall(2024, 10, 27, 2, 30),
            &cet,
            unix(2024, 10, 27, 0, 30, 0),
        ),
        (
            "Eastern summer",
            wall(2024, 7, 1, 12, 0),
            &eastern,
            unix(2024, 7, 1, 16, 0, 0),
        ),
        // West of UTC the same rule lands on the standard offset (02:30Z is
        // before the 07:00Z switch).
        (
            "Eastern gap",
            wall(2024, 3, 10, 2, 30),
            &eastern,
            unix(2024, 3, 10, 7, 30, 0),
        ),
        (
            "Eastern fold",
            wall(2024, 11, 3, 1, 30),
            &eastern,
            unix(2024, 11, 3, 5, 30, 0),
        ),
    ];
    for (name, wall, zone, expected) in cells {
        assert_eq!(zip_times::wall_clock_to_unix(wall, zone), Some(expected), "{name}");
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
        modified_times(bytes, &cet()),
        vec![("spring.txt".to_string(), Some(unix(2024, 3, 31, 0, 30, 0)))]
    );
}
