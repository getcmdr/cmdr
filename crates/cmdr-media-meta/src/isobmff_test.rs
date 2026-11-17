//! The MP4 / MOV walk: Apple's written wall time over `mvhd`'s UTC, 32- and
//! 64-bit sizes, `moov` behind a big `mdat`, and hostile boxes.

use std::io::Cursor;

use crate::isobmff::read_video;
use crate::test_fixtures::{
    CountingReader, apple_meta, at, atom, atom_to_end, atom64, concat, ftyp, mac_seconds, mvhd_v0, mvhd_v1,
    two_hours_ahead,
};
use crate::types::MediaFacts;

fn read(bytes: Vec<u8>) -> MediaFacts {
    read_video(&mut Cursor::new(bytes), &two_hours_ahead).expect("an in-memory read doesn't fail")
}

fn iphone_movie() -> Vec<u8> {
    let recorded_utc = at(2026, 7, 14, 6, 0, 0);
    let moov = atom(
        b"moov",
        &concat(&[
            &mvhd_v0(u32::try_from(mac_seconds(recorded_utc)).unwrap()),
            &apple_meta(&[
                ("com.apple.quicktime.make", "Apple"),
                ("com.apple.quicktime.model", "iPhone 15 Pro"),
                ("com.apple.quicktime.creationdate", "2026-07-14T09:05:30+0200"),
            ]),
        ]),
    );
    concat(&[&ftyp(b"qt  "), &atom(b"mdat", &[0; 64]), &moov])
}

#[test]
fn apple_creation_date_keeps_the_wall_time_it_was_written_in() {
    let facts = read(iphone_movie());
    assert_eq!(
        facts.taken,
        Some(at(2026, 7, 14, 9, 5, 30)),
        "the written time, not mvhd's"
    );
    assert_eq!(facts.make.as_deref(), Some("Apple"));
    assert_eq!(facts.model.as_deref(), Some("iPhone 15 Pro"));
}

#[test]
fn mvhd_utc_is_turned_into_local_time() {
    let utc = at(2025, 12, 31, 23, 30, 0);
    let moov = atom(b"moov", &mvhd_v0(u32::try_from(mac_seconds(utc)).unwrap()));
    let facts = read(concat(&[&ftyp(b"isom"), &moov]));
    assert_eq!(facts.taken, Some(at(2026, 1, 1, 1, 30, 0)));
    assert_eq!(facts.make, None);
}

#[test]
fn mvhd_version_one_has_64_bit_times() {
    let utc = at(2030, 3, 1, 12, 0, 0);
    let facts = read(concat(&[&ftyp(b"isom"), &atom(b"moov", &mvhd_v1(mac_seconds(utc)))]));
    assert_eq!(facts.taken, Some(at(2030, 3, 1, 14, 0, 0)));
}

#[test]
fn an_unreadable_apple_date_falls_back_to_mvhd() {
    let utc = at(2026, 7, 14, 6, 0, 0);
    let moov = atom(
        b"moov",
        &concat(&[
            &mvhd_v0(u32::try_from(mac_seconds(utc)).unwrap()),
            &apple_meta(&[("com.apple.quicktime.creationdate", "sometime in July")]),
        ]),
    );
    assert_eq!(
        read(concat(&[&ftyp(b"qt  "), &moov])).taken,
        Some(at(2026, 7, 14, 8, 0, 0))
    );
}

#[test]
fn a_zero_or_pre_1970_mvhd_is_no_date() {
    let zero = atom(b"moov", &mvhd_v0(0));
    assert_eq!(read(concat(&[&ftyp(b"isom"), &zero])).taken, None);

    // A camera counting from 1970 instead of 1904 lands decades early.
    let early = u32::try_from(mac_seconds(at(1950, 6, 1, 12, 0, 0))).unwrap();
    let wrong_epoch = atom(b"moov", &mvhd_v0(early));
    assert_eq!(read(concat(&[&ftyp(b"isom"), &wrong_epoch])).taken, None);
}

#[test]
fn moov_behind_a_64_bit_mdat_is_found_without_reading_the_media() {
    let utc = at(2026, 7, 14, 6, 0, 0);
    let mdat = atom64(b"mdat", &vec![0xAB; 2 * 1024 * 1024]);
    let moov = atom(b"moov", &mvhd_v0(u32::try_from(mac_seconds(utc)).unwrap()));
    let (mut reader, read) = CountingReader::new(concat(&[&ftyp(b"isom"), &mdat, &moov]));
    let facts = read_video(&mut reader, &two_hours_ahead).unwrap();
    assert_eq!(facts.taken, Some(at(2026, 7, 14, 8, 0, 0)));
    assert!(read.get() < 4096, "read {} bytes; the mdat was read", read.get());
}

#[test]
fn a_box_running_to_the_end_of_the_file_is_walked() {
    let utc = at(2026, 7, 14, 6, 0, 0);
    let moov = atom_to_end(b"moov", &mvhd_v0(u32::try_from(mac_seconds(utc)).unwrap()));
    assert_eq!(
        read(concat(&[&ftyp(b"isom"), &moov])).taken,
        Some(at(2026, 7, 14, 8, 0, 0))
    );
}

#[test]
fn every_truncation_of_a_movie_reads_without_failing() {
    let whole = iphone_movie();
    for len in 0..whole.len() {
        let facts = read_video(&mut Cursor::new(whole[..len].to_vec()), &two_hours_ahead);
        assert!(facts.is_ok(), "cut at {len}: {facts:?}");
    }
}

#[test]
fn hostile_sizes_end_the_walk() {
    // A size below the header's own, a size past the end, a 64-bit size below 16.
    for bytes in [
        concat(&[&3u32.to_be_bytes(), b"moov", &[0; 32]]),
        concat(&[&u32::MAX.to_be_bytes(), b"moov", &mvhd_v0(1)]),
        concat(&[&1u32.to_be_bytes(), b"moov", &8u64.to_be_bytes(), &[0; 32]]),
        concat(&[&1u32.to_be_bytes(), b"moov", &u64::MAX.to_be_bytes(), &[0; 32]]),
        atom(b"moov", &atom(b"meta", &atom(b"keys", &full_keys_claiming(u32::MAX)))),
    ] {
        assert_eq!(read(bytes).taken, None);
    }
}

#[test]
fn apple_dates_read_as_the_wall_time_in_every_shape_a_phone_writes() {
    let cells = [
        ("2026-07-14T09:05:30+0200", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14T09:05:30-0700", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14T09:05:30Z", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14T09:05:30", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14T09:05:30.123+0200", Some(at(2026, 7, 14, 9, 5, 30))),
        ("  2026-07-14T09:05:30+0200\n", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-7-4T9:5:3+0200", Some(at(2026, 7, 4, 9, 5, 3))),
        ("2024-02-29T23:59:59+0100", Some(at(2024, 2, 29, 23, 59, 59))),
        ("2026-07-14 09:05:30+0200", None),
        ("2026-07-14T09:05+0200", None),
        ("2025-02-29T09:05:30+0200", None),
        ("2026-07-14T24:00:00+0200", None),
        ("12026-07-14T09:05:30+0200", None),
        ("sometime in July", None),
        ("", None),
    ];
    for (written, expected) in cells {
        let moov = atom(
            b"moov",
            &concat(&[
                &mvhd_v0(0),
                &apple_meta(&[("com.apple.quicktime.creationdate", written)]),
            ]),
        );
        assert_eq!(read(concat(&[&ftyp(b"qt  "), &moov])).taken, expected, "{written:?}");
    }
}

#[test]
fn an_mvhd_time_counts_up_to_2199_and_no_further() {
    let last = mac_seconds(at(2199, 12, 31, 21, 59, 59));
    let facts = read(concat(&[&ftyp(b"isom"), &atom(b"moov", &mvhd_v1(last))]));
    assert_eq!(facts.taken, Some(at(2199, 12, 31, 23, 59, 59)));

    let past = mac_seconds(at(2200, 1, 1, 0, 0, 0));
    for seconds in [past, i64::MAX.unsigned_abs() / 1000, i64::MAX.unsigned_abs(), u64::MAX] {
        let facts = read(concat(&[&ftyp(b"isom"), &atom(b"moov", &mvhd_v1(seconds))]));
        assert_eq!(facts.taken, None, "{seconds}");
    }
}

/// A `keys` body claiming `count` keys and holding none.
fn full_keys_claiming(count: u32) -> Vec<u8> {
    concat(&[&[0; 4], &count.to_be_bytes()])
}
