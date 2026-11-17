//! The MKV / WebM walk: `DATE_RECORDED` over `DateUTC`, `SeekHead` to `Tags`
//! past the clusters, unknown sizes, and hostile elements.

use std::io::Cursor;

use crate::ebml::read_matroska;
use crate::test_fixtures::{
    CountingReader, at, cluster, concat, ebml_header, el, el_short, el_uint, el_unknown, info_with_date,
    matroska_nanos, seek_head, tag_at, two_hours_ahead,
};
use crate::types::MediaFacts;

const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TAGS: u32 = 0x1254_C367;

fn read(bytes: Vec<u8>) -> MediaFacts {
    read_matroska(&mut Cursor::new(bytes), &two_hours_ahead).expect("an in-memory read doesn't fail")
}

fn segment(children: &[&[u8]]) -> Vec<u8> {
    concat(&[&ebml_header(), &el(SEGMENT, &concat(children))])
}

#[test]
fn date_utc_is_turned_into_local_time() {
    let utc = at(2026, 7, 14, 7, 5, 30);
    let facts = read(segment(&[&info_with_date(matroska_nanos(utc))]));
    assert_eq!(facts.taken, Some(at(2026, 7, 14, 9, 5, 30)));
}

#[test]
fn date_recorded_wins_and_keeps_its_wall_time() {
    let utc = at(2026, 7, 14, 7, 5, 30);
    let tags = el(TAGS, &tag_at(50, &[("DATE_RECORDED", "2024-02-29 18:00:01.250")]));
    let facts = read(segment(&[&info_with_date(matroska_nanos(utc)), &tags]));
    assert_eq!(facts.taken, Some(at(2024, 2, 29, 18, 0, 1)));
}

#[test]
fn simple_tags_fill_the_text_fields() {
    let tags = el(
        TAGS,
        &concat(&[
            &tag_at(
                50,
                &[("TITLE", "Discovery"), ("ARTIST", "Daft Punk"), ("GENRE", "House")],
            ),
            &tag_at(
                30,
                &[
                    ("TITLE", "Digital Love"),
                    ("PART_NUMBER", "3"),
                    ("COMMENT", "Remastered"),
                ],
            ),
            &tag_at(50, &[("DATE_RELEASED", "2001-03-12")]),
        ]),
    );
    let facts = read(segment(&[&tags]));
    assert_eq!(
        facts.title.as_deref(),
        Some("Digital Love"),
        "the track's title, not the album's"
    );
    assert_eq!(facts.album.as_deref(), Some("Discovery"));
    assert_eq!(facts.artist.as_deref(), Some("Daft Punk"));
    assert_eq!(facts.genre.as_deref(), Some("House"));
    assert_eq!(facts.comment.as_deref(), Some("Remastered"));
    assert_eq!(facts.track, Some(3));
    assert_eq!(facts.year, Some(2001));
}

#[test]
fn ffmpeg_style_untargeted_tags_are_read() {
    // ffmpeg writes every tag without a TargetTypeValue (which defaults to 50).
    let tag = el(
        0x7373,
        &concat(&[&el(0x63C0, &[]), &crate::test_fixtures::simple_tag("TITLE", "Holiday")]),
    );
    let facts = read(segment(&[&el(TAGS, &tag)]));
    assert_eq!(facts.title.as_deref(), Some("Holiday"));
}

#[test]
fn a_tag_aimed_at_one_track_is_not_the_files() {
    let aimed = el(
        0x7373,
        &concat(&[
            &el(0x63C0, &el_uint(0x63C5, 12345)),
            &crate::test_fixtures::simple_tag("TITLE", "English audio"),
        ]),
    );
    assert_eq!(read(segment(&[&el(TAGS, &aimed)])).title, None);
}

#[test]
fn seek_head_finds_tags_past_the_clusters_without_reading_them() {
    let info = info_with_date(matroska_nanos(at(2026, 1, 1, 0, 0, 0)));
    let clusters = concat(&[&cluster(400_000), &cluster(400_000)]);
    let tags = el(TAGS, &tag_at(50, &[("DATE_RECORDED", "2025-12-24 17:30:00")]));
    // Positions are from the segment's data start; the seek head's own size is fixed.
    let placeholder = seek_head(&[(INFO, 0), (TAGS, 0)]);
    let info_at = placeholder.len() as u64;
    let tags_at = info_at + (info.len() + clusters.len()) as u64;
    let head = seek_head(&[(INFO, info_at), (TAGS, tags_at)]);
    let (mut reader, read) = CountingReader::new(segment(&[&head, &info, &clusters, &tags]));
    let facts = read_matroska(&mut reader, &two_hours_ahead).unwrap();
    assert_eq!(facts.taken, Some(at(2025, 12, 24, 17, 30, 0)));
    assert!(read.get() < 4096, "read {} bytes; the clusters were read", read.get());
}

#[test]
fn an_unknown_size_segment_runs_to_the_end_of_the_file() {
    let utc = at(2026, 7, 14, 7, 5, 30);
    let bytes = concat(&[
        &ebml_header(),
        &el_unknown(SEGMENT, &info_with_date(matroska_nanos(utc))),
    ]);
    assert_eq!(read(bytes).taken, Some(at(2026, 7, 14, 9, 5, 30)));
}

#[test]
fn an_unknown_size_cluster_ends_the_walk() {
    let live = el_unknown(0x1F43_B675, &[0xAB; 64]);
    let tags = el(TAGS, &tag_at(50, &[("TITLE", "never reached")]));
    let bytes = concat(&[&ebml_header(), &el_unknown(SEGMENT, &concat(&[&live, &tags]))]);
    assert_eq!(read(bytes).title, None);
}

#[test]
fn zero_and_garbage_dates_are_absent() {
    assert_eq!(read(segment(&[&info_with_date(0)])).taken, None);
    assert_eq!(read(segment(&[&info_with_date(i64::MIN)])).taken, None);
    assert_eq!(read(segment(&[&info_with_date(i64::MAX)])).taken, None);
    let tags = el(TAGS, &tag_at(50, &[("DATE_RECORDED", "0000-00-00 00:00:00")]));
    assert_eq!(read(segment(&[&tags])).taken, None);
}

#[test]
fn every_truncation_reads_without_failing() {
    let tags = el(
        TAGS,
        &tag_at(50, &[("DATE_RECORDED", "2025-12-24 17:30:00"), ("TITLE", "x")]),
    );
    let whole = segment(&[&info_with_date(matroska_nanos(at(2026, 1, 1, 0, 0, 0))), &tags]);
    for len in 0..whole.len() {
        let facts = read_matroska(&mut Cursor::new(whole[..len].to_vec()), &two_hours_ahead);
        assert!(facts.is_ok(), "cut at {len}: {facts:?}");
    }
}

#[test]
fn date_recorded_reads_in_every_shape_matroska_writers_use() {
    let cells = [
        ("2026-07-14 09:05:30", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14 09:05:30.123", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14T09:05:30", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14T09:05:30Z", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14 09:05:30+02:00", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14 09:05:30\0", Some(at(2026, 7, 14, 9, 5, 30))),
        (" 2026-07-14 09:05:30\0\0 ", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-7-4 9:5:3", Some(at(2026, 7, 4, 9, 5, 3))),
        ("2026-07-14  09:05:30", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2026-07-14", None),
        ("2026-07-14 09:05", None),
        ("2025-02-29 09:05:30", None),
        ("2026-07-14 24:00:00", None),
        ("12026-07-14 09:05:30", None),
        ("July 14th", None),
    ];
    for (written, expected) in cells {
        let tags = el(TAGS, &tag_at(50, &[("DATE_RECORDED", written)]));
        assert_eq!(read(segment(&[&tags])).taken, expected, "{written:?}");
    }
}

#[test]
fn a_date_utc_counts_up_to_2199_and_no_further() {
    let last = matroska_nanos(at(2199, 12, 31, 21, 59, 59));
    assert_eq!(
        read(segment(&[&info_with_date(last)])).taken,
        Some(at(2199, 12, 31, 23, 59, 59))
    );
    let past = matroska_nanos(at(2200, 1, 1, 0, 0, 0));
    assert_eq!(read(segment(&[&info_with_date(past)])).taken, None);
}

#[test]
fn hostile_elements_end_the_walk() {
    for bytes in [
        // Not EBML at all.
        b"RIFF\0\0\0\0WAVE".to_vec(),
        // An id longer than four bytes.
        concat(&[&ebml_header(), &[0x00, 0x00, 0x80]]),
        // A seek head pointing back at itself, and past the end.
        segment(&[&seek_head(&[(0x114D_9B74, 0), (TAGS, u64::MAX), (INFO, 1 << 40)])]),
        // A string claiming a petabyte.
        segment(&[&el(
            TAGS,
            &el(
                0x7373,
                &el(0x67C8, &concat(&[&[0x45, 0xA3], &[0x01, 0x03, 0, 0, 0, 0, 0, 0]])),
            ),
        )]),
        // A one-byte size of zero everywhere.
        segment(&[&el_short(INFO, &[]), &el_short(TAGS, &[])]),
    ] {
        assert_eq!(read(bytes), MediaFacts::default());
    }
}
