//! Byte builders and readers for the readers' tests: every fixture is built in
//! code, so each one says what it holds.

use std::cell::Cell;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::ops::Range;
use std::rc::Rc;

use jiff::SignedDuration;
use jiff::civil::DateTime;

/// A wall-clock time, for expectations.
pub fn at(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> DateTime {
    DateTime::new(year, month, day, hour, minute, second, 0).expect("a valid test date")
}

/// The test machine's zone: two hours ahead of UTC, whatever the real one is.
pub fn two_hours_ahead(utc: DateTime) -> DateTime {
    utc.checked_add(SignedDuration::from_hours(2))
        .expect("a test date two hours on")
}

/// `parts` back to back.
pub fn concat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

// ── ISOBMFF ───────────────────────────────────────────────────────────────────

/// A box with a 32-bit size.
pub fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = u32::try_from(body.len() + 8).expect("a test box fits 32 bits");
    concat(&[&size.to_be_bytes(), kind, body])
}

/// A box with a 64-bit size (`size == 1`, then the real size).
pub fn atom64(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = (body.len() + 16) as u64;
    concat(&[&1u32.to_be_bytes(), kind, &size.to_be_bytes(), body])
}

/// A box whose size is 0: it runs to the end of the file.
pub fn atom_to_end(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    concat(&[&0u32.to_be_bytes(), kind, body])
}

/// A full box's body: version, three flag bytes, then `body`.
pub fn full(version: u8, body: &[u8]) -> Vec<u8> {
    concat(&[&[version, 0, 0, 0], body])
}

/// Seconds from 1904-01-01 UTC to `utc`, as `mvhd` counts.
pub fn mac_seconds(utc: DateTime) -> u64 {
    u64::try_from(at(1904, 1, 1, 0, 0, 0).duration_until(utc).as_secs()).expect("after 1904")
}

/// `mvhd` version 0: 32-bit creation and modification times.
pub fn mvhd_v0(creation: u32) -> Vec<u8> {
    let mut body = concat(&[
        &creation.to_be_bytes(),
        &creation.to_be_bytes(),
        &1000u32.to_be_bytes(),
        &0u32.to_be_bytes(),
    ]);
    body.resize(body.len() + 80, 0);
    atom(b"mvhd", &full(0, &body))
}

/// `mvhd` version 1: 64-bit times.
pub fn mvhd_v1(creation: u64) -> Vec<u8> {
    let mut body = concat(&[
        &creation.to_be_bytes(),
        &creation.to_be_bytes(),
        &1000u32.to_be_bytes(),
        &0u64.to_be_bytes(),
    ]);
    body.resize(body.len() + 80, 0);
    atom(b"mvhd", &full(1, &body))
}

/// QuickTime's `moov/meta`: a handler, the `keys` list, and an `ilst` whose
/// items name their key by 1-based index, as an iPhone writes it.
pub fn apple_meta(entries: &[(&str, &str)]) -> Vec<u8> {
    let hdlr = atom(b"hdlr", &full(0, &concat(&[&[0; 4], b"mdta", &[0; 12], &[0]])));
    let mut keys_body = full(0, &(entries.len() as u32).to_be_bytes());
    for (key, _) in entries {
        keys_body.extend(atom(b"mdta", key.as_bytes()));
    }
    let keys = atom(b"keys", &keys_body);
    let mut ilst_body = Vec::new();
    for (index, (_, value)) in entries.iter().enumerate() {
        let data = atom(
            b"data",
            &concat(&[&1u32.to_be_bytes(), &0u32.to_be_bytes(), value.as_bytes()]),
        );
        ilst_body.extend(atom(&(index as u32 + 1).to_be_bytes(), &data));
    }
    let ilst = atom(b"ilst", &ilst_body);
    atom(b"meta", &concat(&[&hdlr, &keys, &ilst]))
}

/// An `ftyp` box with `brand`.
pub fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
    atom(b"ftyp", &concat(&[brand, &0u32.to_be_bytes(), brand, b"isom"]))
}

// ── EBML ──────────────────────────────────────────────────────────────────────

/// An element id's bytes (ids keep their length marker, so they're written as is).
fn ebml_id(id: u32) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let skip = bytes.iter().take_while(|b| **b == 0).count();
    bytes[skip..].to_vec()
}

/// An element with an eight-byte size.
pub fn el(id: u32, body: &[u8]) -> Vec<u8> {
    let size = (body.len() as u64) | (1 << 56);
    concat(&[&ebml_id(id), &size.to_be_bytes(), body])
}

/// An element with a one-byte size (bodies under 127 bytes).
pub fn el_short(id: u32, body: &[u8]) -> Vec<u8> {
    let size = u8::try_from(body.len()).expect("a short body") | 0x80;
    concat(&[&ebml_id(id), &[size], body])
}

/// An element whose size is "unknown" (all ones), as a live recorder writes.
pub fn el_unknown(id: u32, body: &[u8]) -> Vec<u8> {
    concat(&[&ebml_id(id), &[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], body])
}

/// A big-endian unsigned integer element, eight bytes.
pub fn el_uint(id: u32, value: u64) -> Vec<u8> {
    el(id, &value.to_be_bytes())
}

/// A Matroska `SimpleTag`.
pub fn simple_tag(name: &str, value: &str) -> Vec<u8> {
    el(
        0x67C8,
        &concat(&[&el(0x45A3, name.as_bytes()), &el(0x4487, value.as_bytes())]),
    )
}

/// A Matroska `Tag` at `level` (`TargetTypeValue`) holding `tags`.
pub fn tag_at(level: u64, tags: &[(&str, &str)]) -> Vec<u8> {
    let mut body = el(0x63C0, &el_uint(0x68CA, level));
    for (name, value) in tags {
        body.extend(simple_tag(name, value));
    }
    el(0x7373, &body)
}

/// The EBML header a WebM file starts with.
pub fn ebml_header() -> Vec<u8> {
    el(0x1A45_DFA3, &el(0x4282, b"webm"))
}

/// `Info` with a `DateUTC` (nanoseconds since 2001-01-01 UTC).
pub fn info_with_date(nanos: i64) -> Vec<u8> {
    el(
        0x1549_A966,
        &concat(&[&el_uint(0x2AD7B1, 1_000_000), &el(0x4461, &nanos.to_be_bytes())]),
    )
}

/// Nanoseconds from 2001-01-01 UTC to `utc`, as `DateUTC` counts.
pub fn matroska_nanos(utc: DateTime) -> i64 {
    i64::try_from(at(2001, 1, 1, 0, 0, 0).duration_until(utc).as_nanos()).expect("in range")
}

/// A `Cluster` of `len` bytes of frames nobody should read.
pub fn cluster(len: usize) -> Vec<u8> {
    el(0x1F43_B675, &vec![0xAB; len])
}

/// A `SeekHead` pointing `id`s at their offsets from the segment's data start.
pub fn seek_head(entries: &[(u32, u64)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (id, position) in entries {
        body.extend(el(
            0x4DBB,
            &concat(&[&el(0x53AB, &ebml_id(*id)), &el_uint(0x53AC, *position)]),
        ));
    }
    el(0x114D_9B74, &body)
}

// ── Photos ────────────────────────────────────────────────────────────────────

/// A minimal JPEG whose EXIF (an APP1 segment, written by `kamadak-exif`) holds
/// `fields`: SOI, APP1, a tiny scan, EOI.
pub fn exif_jpeg(fields: &[exif::Field]) -> Vec<u8> {
    let mut writer = exif::experimental::Writer::new();
    for field in fields {
        writer.push_field(field);
    }
    let mut block = Cursor::new(Vec::new());
    writer
        .write(&mut block, false)
        .expect("kamadak-exif writes the fixture's EXIF");
    let payload = concat(&[b"Exif\0\0", block.get_ref()]);
    let length = u16::try_from(payload.len() + 2).expect("a fixture's EXIF fits one segment");
    concat(&[
        &[0xFF, 0xD8, 0xFF, 0xE1],
        &length.to_be_bytes(),
        &payload,
        &[0xFF, 0xDA, 0x00, 0x08, 1, 1, 0, 0, 63, 0],
        &[0x55; 16],
        &[0xFF, 0xD9],
    ])
}

// ── Tagged audio, written by lofty ────────────────────────────────────────────

/// Three silent MPEG-1 Layer III frames (128 kbps, 44.1 kHz): enough for lofty
/// to take the bytes for an MP3.
pub fn mp3_frames() -> Vec<u8> {
    let mut frame = vec![0u8; 417];
    frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
    frame.repeat(3)
}

/// `fLaC` and a lone STREAMINFO block (44.1 kHz, stereo, 16 bits).
pub fn flac_stream() -> Vec<u8> {
    let mut info = vec![0u8; 34];
    info[..4].copy_from_slice(&[0x10, 0x00, 0x10, 0x00]);
    info[10..14].copy_from_slice(&[0x0A, 0xC4, 0x42, 0xF0]);
    concat(&[b"fLaC", &[0x80, 0, 0, 34], &info])
}

/// An audio-only MP4 (`M4A `) with a `moov` and an empty `mdat`.
pub fn m4a_file() -> Vec<u8> {
    concat(&[&ftyp(b"M4A "), &atom(b"moov", &mvhd_v0(0)), &atom(b"mdat", &[])])
}

/// The tag every audio fixture carries.
pub fn music_tag(tag_type: lofty::tag::TagType) -> lofty::tag::Tag {
    use lofty::tag::{Accessor, ItemKey, Tag};
    let mut tag = Tag::new(tag_type);
    tag.set_title("Digital Love".to_owned());
    tag.set_artist("Daft Punk".to_owned());
    tag.set_album("Discovery".to_owned());
    tag.insert_text(ItemKey::AlbumArtist, "Daft Punk".to_owned());
    tag.set_genre("House".to_owned());
    tag.set_comment("Remastered".to_owned());
    tag.set_track(3);
    tag.set_disk(1);
    tag.set_date(lofty::tag::items::Timestamp {
        year: 2001,
        ..Default::default()
    });
    tag
}

/// A front cover of `len` bytes of a pattern a test can spot.
pub fn cover(len: usize) -> lofty::picture::Picture {
    use lofty::picture::{MimeType, Picture, PictureType};
    Picture::unchecked(vec![0xC0; len])
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::Jpeg)
        .build()
}

/// `file` with `tag` written into it by lofty.
pub fn tagged(file: Vec<u8>, tag: &lofty::tag::Tag, options: lofty::config::WriteOptions) -> Vec<u8> {
    use lofty::tag::TagExt;
    let mut cursor = Cursor::new(file);
    tag.save_to(&mut cursor, options)
        .expect("lofty writes the fixture's tag");
    cursor.into_inner()
}

/// An MP3 with an ID3v2 tag holding [`music_tag`] and a front cover of
/// `cover_len` bytes.
pub fn id3_mp3_with_cover(cover_len: usize) -> Vec<u8> {
    let mut tag = music_tag(lofty::tag::TagType::Id3v2);
    tag.push_picture(cover(cover_len));
    tagged(mp3_frames(), &tag, lofty::config::WriteOptions::default())
}

// ── Readers ───────────────────────────────────────────────────────────────────

/// A `Read + Seek` over bytes that counts what it hands out, and serves `holes`
/// as zeros without counting them, the way the block reader skips cover art.
pub struct CountingReader {
    inner: Cursor<Vec<u8>>,
    holes: Vec<Range<u64>>,
    read: Rc<Cell<u64>>,
}

impl CountingReader {
    /// Over `bytes`, with no holes; `counter` sees every byte read.
    pub fn new(bytes: Vec<u8>) -> (Self, Rc<Cell<u64>>) {
        let read = Rc::new(Cell::new(0));
        let reader = Self {
            inner: Cursor::new(bytes),
            holes: Vec::new(),
            read: Rc::clone(&read),
        };
        (reader, read)
    }

    /// The same reader, serving `holes` as zeros.
    pub fn with_holes(mut self, holes: Vec<Range<u64>>) -> Self {
        self.holes = holes;
        self
    }
}

impl Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let start = self.inner.position();
        let n = self.inner.read(buf)?;
        for (i, byte) in buf[..n].iter_mut().enumerate() {
            let offset = start + i as u64;
            if self.holes.iter().any(|hole| hole.contains(&offset)) {
                *byte = 0;
            } else {
                self.read.set(self.read.get() + 1);
            }
        }
        Ok(n)
    }
}

impl Seek for CountingReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}
