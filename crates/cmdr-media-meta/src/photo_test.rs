//! Photos: EXIF located by our own container walks (JPEG, PNG, WebP, TIFF and
//! its RAW cousins) and read by `kamadak-exif`, with the container's own size as
//! the fallback. HEIF rides `kamadak-exif`'s own container code and its tests:
//! its writer makes TIFF blocks, not ISO-BMFF boxes.

use std::io::Cursor;

use exif::experimental::Writer;
use exif::{Field, In, Rational, Tag, Value};

use crate::photo::read_photo;
use crate::test_fixtures::{CountingReader, at, concat};
use crate::types::{MediaFacts, ReadError};

// ── Fixtures ──────────────────────────────────────────────────────────────────

fn ascii(tag: Tag, text: &str) -> Field {
    Field {
        tag,
        ifd_num: In::PRIMARY,
        value: Value::Ascii(vec![text.as_bytes().to_vec()]),
    }
}

fn rational(tag: Tag, num: u32, denom: u32) -> Field {
    Field {
        tag,
        ifd_num: In::PRIMARY,
        value: Value::Rational(vec![Rational { num, denom }]),
    }
}

fn short(tag: Tag, n: u16) -> Field {
    Field {
        tag,
        ifd_num: In::PRIMARY,
        value: Value::Short(vec![n]),
    }
}

fn long(tag: Tag, n: u32) -> Field {
    Field {
        tag,
        ifd_num: In::PRIMARY,
        value: Value::Long(vec![n]),
    }
}

/// An EXIF block (a big-endian TIFF structure) holding `fields`, as a camera
/// embeds it.
fn exif_block(fields: &[Field]) -> Vec<u8> {
    exif_block_in(fields, false)
}

fn exif_block_in(fields: &[Field], little_endian: bool) -> Vec<u8> {
    let mut writer = Writer::new();
    for field in fields {
        writer.push_field(field);
    }
    let mut buf = Cursor::new(Vec::new());
    writer.write(&mut buf, little_endian).unwrap();
    buf.into_inner()
}

fn iphone_shot() -> Vec<Field> {
    vec![
        ascii(Tag::Make, "Apple"),
        ascii(Tag::Model, "iPhone 15 Pro"),
        short(Tag::Orientation, 6),
        ascii(Tag::DateTime, "2026:07:15 10:00:00"),
        ascii(Tag::DateTimeDigitized, "2026:07:14 09:05:31"),
        ascii(Tag::DateTimeOriginal, "2026:07:14 09:05:30"),
        ascii(Tag::LensModel, "iPhone 15 Pro back triple camera 6.765mm f/1.78"),
        short(Tag::PhotographicSensitivity, 64),
        rational(Tag::FocalLength, 6765, 1000),
        long(Tag::PixelXDimension, 4032),
        long(Tag::PixelYDimension, 3024),
    ]
}

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let len = u16::try_from(payload.len() + 2).unwrap();
    concat(&[&[0xFF, marker], &len.to_be_bytes(), payload])
}

/// A baseline frame header: precision, height, width, one component.
fn sof(width: u16, height: u16) -> Vec<u8> {
    segment(
        0xC0,
        &concat(&[&[8], &height.to_be_bytes(), &width.to_be_bytes(), &[1, 1, 0x11, 0]]),
    )
}

/// SOI, `segments`, a scan of `scan_len` bytes, EOI.
fn jpeg(segments: &[Vec<u8>], scan_len: usize) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    for s in segments {
        bytes.extend(s);
    }
    bytes.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
    bytes.extend(vec![0x55; scan_len]);
    bytes.extend([0xFF, 0xD9]);
    bytes
}

fn app1_exif(fields: &[Field]) -> Vec<u8> {
    segment(0xE1, &concat(&[b"Exif\0\0", &exif_block(fields)]))
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    concat(&[&(data.len() as u32).to_be_bytes(), kind, data, &[0; 4]])
}

fn png(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    for c in chunks {
        bytes.extend(c);
    }
    bytes
}

fn ihdr(width: u32, height: u32) -> Vec<u8> {
    png_chunk(
        b"IHDR",
        &concat(&[&width.to_be_bytes(), &height.to_be_bytes(), &[8, 6, 0, 0, 0]]),
    )
}

fn riff_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = concat(&[kind, &(data.len() as u32).to_le_bytes(), data]);
    if data.len() % 2 == 1 {
        chunk.push(0);
    }
    chunk
}

fn webp(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body = concat(&[b"WEBP", &chunks.concat()]);
    concat(&[b"RIFF", &(body.len() as u32).to_le_bytes(), &body])
}

/// VP8X: flags, reserved, then canvas width − 1 and height − 1, 24-bit.
fn vp8x(width: u32, height: u32) -> Vec<u8> {
    let w = (width - 1).to_le_bytes();
    let h = (height - 1).to_le_bytes();
    riff_chunk(b"VP8X", &[0x08, 0, 0, 0, w[0], w[1], w[2], h[0], h[1], h[2]])
}

fn read(bytes: Vec<u8>) -> Result<MediaFacts, ReadError> {
    read_photo(&mut Cursor::new(bytes))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn a_jpeg_gives_every_exif_field() {
    let facts = read(jpeg(&[app1_exif(&iphone_shot()), sof(4032, 3024)], 100)).unwrap();
    assert_eq!(facts.taken, Some(at(2026, 7, 14, 9, 5, 30)), "DateTimeOriginal wins");
    assert_eq!(facts.make.as_deref(), Some("Apple"));
    assert_eq!(facts.model.as_deref(), Some("iPhone 15 Pro"));
    assert_eq!(
        facts.lens.as_deref(),
        Some("iPhone 15 Pro back triple camera 6.765mm f/1.78")
    );
    assert_eq!(facts.iso, Some(64));
    assert_eq!(facts.focal_mm, Some(7), "6.765 mm rounds to 7");
    assert_eq!((facts.width, facts.height), (Some(4032), Some(3024)));
    assert_eq!(facts.orientation, Some(6));
    assert_eq!(facts.title, None);
}

#[test]
fn the_date_falls_back_to_digitized_then_modified() {
    let digitized = [
        ascii(Tag::DateTimeDigitized, "2026:07:14 09:05:31"),
        ascii(Tag::DateTime, "2026:07:15 10:00:00"),
    ];
    assert_eq!(
        read(jpeg(&[app1_exif(&digitized)], 0)).unwrap().taken,
        Some(at(2026, 7, 14, 9, 5, 31))
    );
    let modified = [ascii(Tag::DateTime, "2026:07:15 10:00:00")];
    assert_eq!(
        read(jpeg(&[app1_exif(&modified)], 0)).unwrap().taken,
        Some(at(2026, 7, 15, 10, 0, 0))
    );
}

#[test]
fn blank_and_zero_dates_are_absent() {
    for blank in ["    :  :     :  :  ", "0000:00:00 00:00:00", "2026:13:45 25:61:00", ""] {
        let fields = [ascii(Tag::DateTimeOriginal, blank), ascii(Tag::Make, "Canon")];
        let facts = read(jpeg(&[app1_exif(&fields)], 0)).unwrap();
        assert_eq!(facts.taken, None, "{blank:?}");
        assert_eq!(facts.make.as_deref(), Some("Canon"));
    }
}

#[test]
fn out_of_range_numbers_are_absent() {
    let fields = [
        short(Tag::Orientation, 9),
        rational(Tag::FocalLength, 50, 0),
        short(Tag::PhotographicSensitivity, 0),
    ];
    let facts = read(jpeg(&[app1_exif(&fields)], 0)).unwrap();
    assert_eq!((facts.orientation, facts.focal_mm, facts.iso), (None, None, None));
}

#[test]
fn a_jpeg_without_exif_still_has_its_size_and_never_reads_the_scan() {
    let (mut reader, read) = CountingReader::new(jpeg(&[segment(0xE0, b"JFIF\0\x01\x02"), sof(1920, 1080)], 2_000_000));
    let facts = read_photo(&mut reader).unwrap();
    assert_eq!((facts.width, facts.height), (Some(1920), Some(1080)));
    assert_eq!(facts.taken, None);
    assert!(read.get() < 4096, "read {} bytes; the scan was read", read.get());
}

#[test]
fn the_frame_size_beats_a_stale_exif_size() {
    let facts = read(jpeg(&[app1_exif(&iphone_shot()), sof(2000, 1500)], 0)).unwrap();
    assert_eq!(
        (facts.width, facts.height),
        (Some(2000), Some(1500)),
        "a crop kept the old EXIF size"
    );
}

#[test]
fn a_png_with_an_exif_chunk() {
    let bytes = png(&[
        ihdr(800, 600),
        png_chunk(
            b"eXIf",
            &exif_block(&[ascii(Tag::DateTimeOriginal, "2025:01:02 03:04:05")]),
        ),
        png_chunk(b"IDAT", &[0; 64]),
        png_chunk(b"IEND", &[]),
    ]);
    let facts = read(bytes).unwrap();
    assert_eq!(facts.taken, Some(at(2025, 1, 2, 3, 4, 5)));
    assert_eq!((facts.width, facts.height), (Some(800), Some(600)));
}

#[test]
fn a_png_walk_stops_at_the_image_data() {
    let mut chunks = vec![ihdr(10, 10)];
    for _ in 0..2000 {
        chunks.push(png_chunk(b"IDAT", &[0x55; 8192]));
    }
    let (mut reader, read) = CountingReader::new(png(&chunks));
    assert_eq!(read_photo(&mut reader).unwrap().width, Some(10));
    assert!(read.get() < 4096, "read {} bytes", read.get());
}

#[test]
fn a_webp_with_an_exif_chunk() {
    let block = exif_block(&[ascii(Tag::Model, "Pixel 9")]);
    for payload in [block.clone(), concat(&[b"Exif\0\0", &block])] {
        let bytes = webp(&[
            vp8x(1024, 768),
            riff_chunk(b"VP8 ", &[0; 101]),
            riff_chunk(b"EXIF", &payload),
        ]);
        let facts = read(bytes).unwrap();
        assert_eq!(facts.model.as_deref(), Some("Pixel 9"));
        assert_eq!((facts.width, facts.height), (Some(1024), Some(768)));
    }
}

#[test]
fn a_tiff_raw_reads_its_head_only() {
    let mut bytes = exif_block(&iphone_shot());
    bytes.extend(vec![0x55; 3 * 1024 * 1024]);
    let (mut reader, read) = CountingReader::new(bytes);
    let facts = read_photo(&mut reader).unwrap();
    assert_eq!(facts.taken, Some(at(2026, 7, 14, 9, 5, 30)));
    assert!(
        read.get() < 300 * 1024,
        "read {} bytes; the raw data was read",
        read.get()
    );
}

#[test]
fn olympus_and_panasonic_raw_signatures_read_as_tiff() {
    let little = exif_block_in(&[ascii(Tag::Make, "OLYMPUS")], true);
    let big = exif_block_in(&[ascii(Tag::Make, "OLYMPUS")], false);
    for (mut block, magic) in [
        (little.clone(), b"RO"),
        (little.clone(), b"SR"),
        (little, b"U\0"),
        (big, b"OR"),
    ] {
        block[2..4].copy_from_slice(magic);
        assert_eq!(read(block).unwrap().make.as_deref(), Some("OLYMPUS"), "{magic:?}");
    }
}

#[test]
fn exif_dates_read_as_the_cameras_wall_clock() {
    let cells = [
        ("2026:07:14 09:05:30", Some(at(2026, 7, 14, 9, 5, 30))),
        ("2024:02:29 23:59:59", Some(at(2024, 2, 29, 23, 59, 59))),
        ("2026:07:14 09:05:60", Some(at(2026, 7, 14, 9, 5, 59))),
        ("1900:01:01 00:00:00", Some(at(1900, 1, 1, 0, 0, 0))),
        ("2025:02:29 09:05:30", None),
        ("2026:07:14 24:00:00", None),
    ];
    for (written, expected) in cells {
        let fields = [ascii(Tag::DateTimeOriginal, written)];
        let facts = read(jpeg(&[app1_exif(&fields)], 0)).unwrap();
        assert_eq!(facts.taken, expected, "{written:?}");
    }
}

#[test]
fn unknown_content_is_not_supported() {
    assert_eq!(read(b"GIF89a....".to_vec()), Err(ReadError::NotSupported));
    assert_eq!(read(Vec::new()), Err(ReadError::NotSupported));
}

#[test]
fn every_truncation_reads_without_panicking() {
    let wholes = [
        jpeg(&[app1_exif(&iphone_shot()), sof(4032, 3024)], 32),
        png(&[ihdr(8, 8), png_chunk(b"eXIf", &exif_block(&iphone_shot()))]),
        webp(&[vp8x(8, 8), riff_chunk(b"EXIF", &exif_block(&iphone_shot()))]),
        exif_block(&iphone_shot()),
    ];
    for whole in wholes {
        for len in 0..whole.len() {
            let _ = read(whole[..len].to_vec());
        }
    }
}

#[test]
fn hostile_lengths_end_the_walk() {
    // A segment length below its own two bytes, a PNG chunk claiming 4 GB, a
    // RIFF chunk claiming more than the file.
    let short_segment = concat(&[&[0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01], &[0; 16]]);
    let huge_chunk = png(&[concat(&[&u32::MAX.to_be_bytes(), b"eXIf", &[0; 16]])]);
    let riff_overflow = webp(&[concat(&[b"EXIF", &u32::MAX.to_le_bytes(), &[0; 16]])]);
    for bytes in [short_segment, huge_chunk, riff_overflow] {
        let facts = read(bytes);
        assert!(matches!(facts, Ok(ref f) if f.taken.is_none()), "{facts:?}");
    }
}
