//! A scratch name has to fit wherever its final name fits, or a write fails on
//! OUR name rather than the user's (ERR-9JY7Q: a byte-capped epub name plus the
//! 46-byte temp suffix, refused by a Synology share as `STATUS_OBJECT_NAME_INVALID`).

use std::path::Path;
use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;

use super::*;

/// The usual filesystem name limit (APFS, ext4, and NTFS in UTF-16 units),
/// which the FINAL names in these tests sit at or past.
const LIMIT: usize = 255;

fn name_of(temp: &StagingTemp) -> String {
    temp.path()
        .file_name()
        .expect("a minted temp always has a filename")
        .to_string_lossy()
        .into_owned()
}

fn temp_for(final_name: &str) -> String {
    name_of(&StagingTemp::mint(&Path::new("/dir").join(final_name), None))
}

fn aside_for(final_name: &str) -> String {
    name_of(&StagingTemp::mint_aside(
        &Path::new("/dir").join(final_name),
        uuid::Uuid::new_v4(),
        None,
    ))
}

fn assert_fits(name: &str) {
    assert!(
        name.len() <= MAX_SCRATCH_NAME_BYTES,
        "{} UTF-8 bytes: {name}",
        name.len()
    );
    let units = name.encode_utf16().count();
    assert!(units <= MAX_SCRATCH_NAME_BYTES, "{units} UTF-16 units: {name}");
}

/// The part of `scratch` in front of `marker`: what it kept of the original name.
fn kept<'a>(scratch: &'a str, marker: &str) -> &'a str {
    &scratch[..scratch.rfind(marker).expect("the marker survives every cut")]
}

/// `kept` is a run of WHOLE grapheme clusters from the start of `original`: no
/// accent stranded without its letter, no emoji split mid-sequence.
fn assert_whole_graphemes(original: &str, kept: &str) {
    assert!(original.starts_with(kept), "{kept:?} isn't a prefix of {original:?}");
    let boundaries: Vec<usize> = original
        .grapheme_indices(true)
        .map(|(at, _)| at)
        .chain([original.len()])
        .collect();
    assert!(
        boundaries.contains(&kept.len()),
        "cut at byte {} splits a grapheme cluster of {original:?}",
        kept.len()
    );
}

#[test]
fn a_short_name_is_kept_whole() {
    let uuid = uuid::Uuid::new_v4();
    let temp = StagingTemp::mint_with_uuid(Path::new("/dir/photo.jpg"), uuid, None);
    assert_eq!(name_of(&temp), format!("photo.jpg.cmdr-tmp-{uuid}"));
}

/// The ERR-9JY7Q shape: a downloader had already byte-capped the name to the
/// limit, so ANY suffix pushes it over.
#[test]
fn a_temp_for_a_255_byte_ascii_name_fits() {
    let original = format!("{}.epub", "a".repeat(LIMIT - 5));
    assert_eq!(original.len(), LIMIT);

    let temp = temp_for(&original);
    assert_fits(&temp);
    assert_whole_graphemes(&original, kept(&temp, STAGING_TEMP_MARKER));
    assert!(is_staging_temp_name(&temp), "got {temp}");

    let aside = aside_for(&original);
    assert_fits(&aside);
    assert!(is_staging_temp_name(&aside), "got {aside}");
}

/// Greek letters are two bytes each in UTF-8, so a name well under 255
/// CHARACTERS is already at the byte limit.
#[test]
fn a_temp_for_a_greek_name_near_the_byte_limit_fits() {
    let original = format!("{}.epub", "αβγδε".repeat(25));
    assert_eq!(original.len(), LIMIT);

    let temp = temp_for(&original);
    assert_fits(&temp);
    assert_whole_graphemes(&original, kept(&temp, STAGING_TEMP_MARKER));
}

/// A UTF-16 unit never costs fewer bytes than its UTF-8 spelling, so a name
/// can't be near the UTF-16 limit while under the byte one. The case that
/// matters is the reverse: a CJK name a Windows server holds happily (200 units,
/// 600 bytes). Its temp must fit there too, and in bytes for a Linux server.
#[test]
fn a_temp_for_a_cjk_name_past_the_byte_limit_fits_both_units() {
    let original = format!("{}.txt", "漢".repeat(200));
    assert!(original.encode_utf16().count() <= LIMIT);

    let temp = temp_for(&original);
    assert_fits(&temp);
    assert_whole_graphemes(&original, kept(&temp, STAGING_TEMP_MARKER));
}

/// An emoji is two UTF-16 units and four bytes; a ZWJ family is five code
/// points that must stay together.
#[test]
fn a_cut_never_splits_an_emoji_sequence() {
    let original = format!("{}.jpg", "👨‍👩‍👧".repeat(20));

    let temp = temp_for(&original);
    assert_fits(&temp);
    let kept = kept(&temp, STAGING_TEMP_MARKER);
    assert!(!kept.is_empty(), "got {temp}");
    assert_whole_graphemes(&original, kept);
}

/// `e` + COMBINING ACUTE is one character on screen and three bytes on disk,
/// which puts the byte budget inside the cluster: the accent goes with its
/// letter or not at all.
#[test]
fn a_cut_never_strands_a_combining_mark() {
    let original = format!("{}.txt", "e\u{301}".repeat(100));

    for scratch in [temp_for(&original), aside_for(&original)] {
        assert_fits(&scratch);
        let marker = if scratch.contains(STAGING_TEMP_MARKER) {
            STAGING_TEMP_MARKER
        } else {
            STAGING_ASIDE_MARKER
        };
        assert_whole_graphemes(&original, kept(&scratch, marker));
    }
}

/// A shortened temp is still ours to every reader: the name test, and the
/// in-flight registry the listing hides by.
#[test]
fn a_shortened_temp_is_still_recognized_and_in_flight() {
    let operation = Arc::new(());
    let original = format!("{}.epub", "ü".repeat(125));
    let temp = StagingTemp::mint(&Path::new("/dir").join(&original), Some(Arc::downgrade(&operation)));
    let name = name_of(&temp);

    assert!(name.len() < original.len() + STAGING_TEMP_MARKER.len(), "got {name}");
    assert!(is_staging_temp_name(&name), "got {name}");
    assert!(is_cmdr_scratch_name(&name), "got {name}");
    assert!(is_staging_temp_in_flight(&name), "got {name}");
    drop(temp);
    assert!(!is_staging_temp_in_flight(&name), "got {name}");
}

/// The real thing: a filesystem with a 255-byte name limit (APFS, ext4) takes
/// the temp for a name that's already at that limit.
#[test]
fn a_real_filesystem_takes_the_temp_for_a_name_at_its_limit() {
    let dir = tempfile::tempdir().unwrap();
    for original in [
        format!("{}.epub", "a".repeat(LIMIT - 5)),
        format!("{}.epub", "αβγδε".repeat(25)),
    ] {
        let temp = StagingTemp::mint(&dir.path().join(&original), None);
        std::fs::write(temp.path(), b"bytes").unwrap_or_else(|e| panic!("{}: {e}", name_of(&temp)));
        std::fs::rename(temp.path(), dir.path().join(&original)).unwrap();
    }
}

/// A reaper clearing one file's leftovers (an archive edit's crashed builds)
/// matches by prefix, so the prefix has to be the one the mint used, shortened
/// or not.
#[test]
fn the_sibling_prefix_matches_every_temp_minted_for_that_name() {
    for original in [
        "photo.jpg".to_string(),
        format!("{}.zip", "a".repeat(LIMIT - 4)),
        format!("{}.zip", "e\u{301}".repeat(100)),
    ] {
        let prefix = temp_sibling_prefix(&original);
        for temp in [
            temp_for(&original),
            temp_sibling_name(&original),
            name_of_path(&temp_sibling_path(&Path::new("/dir").join(&original))),
        ] {
            assert!(temp.starts_with(&prefix), "{temp} doesn't start with {prefix}");
            assert_fits(&temp);
        }
    }
    // A sibling archive's temps stay out of it.
    assert!(!temp_sibling_name("photo.jpg").starts_with(&temp_sibling_prefix("photo.jp")));
}

/// The path form keeps the temp in the final name's directory: a landing
/// rename across directories is one some backends refuse.
#[test]
fn a_temp_sibling_path_stays_in_the_final_names_directory() {
    let path = temp_sibling_path(Path::new("/share/books/long.epub"));
    assert_eq!(path.parent(), Some(Path::new("/share/books")));
    assert!(is_staging_temp_name(&name_of_path(&path)), "got {}", path.display());
}

fn name_of_path(path: &Path) -> String {
    path.file_name()
        .expect("a temp path has a filename")
        .to_string_lossy()
        .into_owned()
}

/// The cap is eCryptfs's 143 bytes (a Synology encrypted share), so a name
/// that fits there gets a temp that fits there too. A name up to 97 bytes keeps
/// its original part whole; one byte more and it gives way.
#[test]
fn a_temp_fits_an_ecryptfs_share_and_a_short_enough_name_stays_whole() {
    assert_eq!(MAX_SCRATCH_NAME_BYTES, 143);
    let room = MAX_SCRATCH_NAME_BYTES - STAGING_TEMP_MARKER.len() - 36;

    let whole = format!("{}.txt", "a".repeat(room - 4));
    let temp = temp_for(&whole);
    assert_eq!(kept(&temp, STAGING_TEMP_MARKER), whole);
    assert_eq!(temp.len(), MAX_SCRATCH_NAME_BYTES);

    let one_more = format!("{}.txt", "a".repeat(room - 3));
    let temp = temp_for(&one_more);
    assert_eq!(kept(&temp, STAGING_TEMP_MARKER).len(), room);
    assert_fits(&temp);

    let at_ecryptfs_limit = format!("{}.epub", "a".repeat(MAX_SCRATCH_NAME_BYTES - 5));
    assert_fits(&temp_for(&at_ecryptfs_limit));
    assert_fits(&aside_for(&at_ecryptfs_limit));
}
