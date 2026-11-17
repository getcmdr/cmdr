//! Photos: our own walks find the EXIF block (JPEG, PNG, WebP, TIFF-based RAW)
//! by seeking past image data, and `kamadak-exif` parses it. Its own container
//! code reads JPEG scans, PNG `IDAT`s, and whole TIFFs through to find a block,
//! which would spend a file's whole byte budget on pixels; we keep it for HEIF
//! only, whose walk seeks.

use std::io::{self, BufReader, Read, Seek, SeekFrom};

use exif::{Context, Exif, In, Tag, Value};
use jiff::civil::DateTime;

use crate::bytes::{be_u16, be_u32, file_len, le_u32, read_exact_at, read_up_to_at, read_vec_at};
use crate::types::{MediaFacts, ReadError};

/// How much of a TIFF-based RAW we read: its IFDs and EXIF sit at the start,
/// and the pixels after them are never needed.
const TIFF_HEAD_BYTES: usize = 256 * 1024;

/// The most segments or chunks a walk visits before it gives up on the file.
const MAX_STEPS: usize = 4096;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// `NewSubfileType`, which `kamadak-exif` doesn't name: 1 marks IFD0 as a
/// reduced-size preview (DNG, NEF), whose size isn't the photo's.
const NEW_SUBFILE_TYPE: Tag = Tag(Context::Tiff, 0x00FE);

/// What a container walk found: the EXIF block, and the container's own pixel
/// size when it states one.
#[derive(Default)]
struct Found {
    block: Option<Vec<u8>>,
    width: Option<i64>,
    height: Option<i64>,
}

/// Every EXIF field `reader`'s photo carries, by its content (a `.jpg` that's
/// really a PNG reads as a PNG). A photo without EXIF has no fields but its
/// size; content we don't know is `NotSupported`.
pub(crate) fn read_photo<R: Read + Seek>(reader: &mut R) -> Result<MediaFacts, ReadError> {
    let mut head = [0u8; 16];
    let len = read_up_to_at(reader, 0, &mut head).map_err(ReadError::from_io)?;
    let head = &head[..len];
    let found = if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        jpeg(reader)
    } else if head.starts_with(PNG_SIGNATURE) {
        png(reader)
    } else if head.get(..4) == Some(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        webp(reader)
    } else if let Some(signature) = tiff_signature(head) {
        tiff(reader, signature)
    } else if head.get(4..8) == Some(b"ftyp") {
        return heif(reader);
    } else {
        return Err(ReadError::NotSupported);
    }
    .map_err(ReadError::from_io)?;
    let exif = found.block.map(parse_block).transpose()?.flatten();
    let mut facts = exif.as_ref().map(facts_from_exif).unwrap_or_default();
    if let (Some(width), Some(height)) = (found.width, found.height) {
        // The frame's own size beats EXIF's, which an editor's crop may leave stale.
        facts.width = Some(width);
        facts.height = Some(height);
    }
    Ok(facts)
}

/// JPEG: the segments before the scan. `APP1` with `Exif\0\0` is the block, a
/// frame header (`SOFn`) the size. The scan (`SOS`) ends the walk unread.
fn jpeg<R: Read + Seek>(reader: &mut R) -> io::Result<Found> {
    let mut found = Found::default();
    let mut pos = 2;
    for _ in 0..MAX_STEPS {
        let mut marker = [0u8; 4];
        if !read_exact_at(reader, pos, &mut marker)? || marker[0] != 0xFF {
            break;
        }
        match marker[1] {
            // Fill bytes before a marker.
            0xFF => {
                pos += 1;
                continue;
            }
            // Stand-alone markers carry no length.
            0x01 | 0xD0..=0xD7 => {
                pos += 2;
                continue;
            }
            // The scan or the end: nothing after it is metadata we read.
            0xDA | 0xD9 => break,
            _ => {}
        }
        let len = u64::from(be_u16(&marker, 2).unwrap_or(0));
        if len < 2 {
            break;
        }
        let data = pos + 4;
        let data_len = len - 2;
        match marker[1] {
            0xE1 if found.block.is_none() => {
                if let Some(segment) = read_vec_at(reader, data, data_len)?
                    && let Some(block) = segment.strip_prefix(b"Exif\0\0")
                {
                    found.block = Some(block.to_vec());
                }
            }
            // Every SOFn but DHT (C4), JPG (C8), and DAC (CC).
            0xC0..=0xCF if !matches!(marker[1], 0xC4 | 0xC8 | 0xCC) => {
                let mut frame = [0u8; 5];
                if read_exact_at(reader, data, &mut frame)? {
                    found.height = be_u16(&frame, 1).map(i64::from).filter(|h| *h > 0);
                    found.width = be_u16(&frame, 3).map(i64::from).filter(|w| *w > 0);
                }
            }
            _ => {}
        }
        pos = data + data_len;
    }
    Ok(found)
}

/// PNG: the chunks before the image data. `IHDR` is the size, `eXIf` the block.
/// The walk stops at the first `IDAT`: an encoder may split the pixels into
/// thousands of chunks, and walking them would fetch the whole file.
fn png<R: Read + Seek>(reader: &mut R) -> io::Result<Found> {
    let mut found = Found::default();
    let mut pos = PNG_SIGNATURE.len() as u64;
    for _ in 0..MAX_STEPS {
        let mut header = [0u8; 8];
        if !read_exact_at(reader, pos, &mut header)? {
            break;
        }
        let len = u64::from(be_u32(&header, 0).unwrap_or(0));
        let data = pos + 8;
        match &header[4..8] {
            b"IHDR" => {
                let mut size = [0u8; 8];
                if read_exact_at(reader, data, &mut size)? {
                    found.width = be_u32(&size, 0).map(i64::from).filter(|w| *w > 0);
                    found.height = be_u32(&size, 4).map(i64::from).filter(|h| *h > 0);
                }
            }
            b"eXIf" => found.block = read_vec_at(reader, data, len)?,
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        // Data, then a four-byte CRC.
        pos = data + len + 4;
    }
    Ok(found)
}

/// WebP: RIFF chunks. `VP8X` is the canvas size, `EXIF` the block (some writers
/// keep JPEG's `Exif\0\0` in front of it).
fn webp<R: Read + Seek>(reader: &mut R) -> io::Result<Found> {
    let mut found = Found::default();
    let mut riff = [0u8; 12];
    if !read_exact_at(reader, 0, &mut riff)? {
        return Ok(found);
    }
    let end = (u64::from(le_u32(&riff, 4).unwrap_or(0)) + 8).min(file_len(reader)?);
    let mut pos = 12;
    for _ in 0..MAX_STEPS {
        let mut header = [0u8; 8];
        if pos + 8 > end || !read_exact_at(reader, pos, &mut header)? {
            break;
        }
        let len = u64::from(le_u32(&header, 4).unwrap_or(0));
        let data = pos + 8;
        if data + len > end {
            break;
        }
        match &header[..4] {
            b"VP8X" => {
                let mut canvas = [0u8; 10];
                if read_exact_at(reader, data, &mut canvas)? {
                    let u24 = |at: usize| {
                        i64::from(canvas[at]) | i64::from(canvas[at + 1]) << 8 | i64::from(canvas[at + 2]) << 16
                    };
                    found.width = Some(u24(4) + 1);
                    found.height = Some(u24(7) + 1);
                }
            }
            b"EXIF" => {
                found.block = read_vec_at(reader, data, len)?.map(|block| match block.strip_prefix(b"Exif\0\0") {
                    Some(rest) => rest.to_vec(),
                    None => block,
                });
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        pos = data + len + (len % 2);
    }
    Ok(found)
}

/// The byte order a TIFF-based file uses, with the RAW signatures that differ
/// from TIFF's `42` only in their magic number: Olympus ORF (`RO`, `SR`) and
/// Panasonic RW2 (`U`).
fn tiff_signature(head: &[u8]) -> Option<[u8; 4]> {
    match head.get(..4)? {
        b"II*\0" | b"IIRO" | b"IISR" | b"IIU\0" => Some(*b"II*\0"),
        b"MM\0*" | b"MMOR" => Some(*b"MM\0*"),
        _ => None,
    }
}

/// TIFF and its RAW cousins: the whole file is the EXIF block, with offsets from
/// its start. Its head is enough; a value pointing past it is skipped.
fn tiff<R: Read + Seek>(reader: &mut R, signature: [u8; 4]) -> io::Result<Found> {
    let mut head = vec![0u8; TIFF_HEAD_BYTES];
    let len = read_up_to_at(reader, 0, &mut head)?;
    head.truncate(len);
    if let Some(magic) = head.get_mut(..4) {
        magic.copy_from_slice(&signature);
    }
    Ok(Found {
        block: Some(head),
        ..Found::default()
    })
}

/// HEIF / HEIC / AVIF: `kamadak-exif`'s own walk, which seeks past `mdat` and
/// reads the `meta` box and the EXIF item.
fn heif<R: Read + Seek>(reader: &mut R) -> Result<MediaFacts, ReadError> {
    reader.seek(SeekFrom::Start(0)).map_err(ReadError::from_io)?;
    let mut buffered = BufReader::new(reader);
    let mut parser = exif::Reader::new();
    parser.continue_on_error(true);
    match parser.read_from_container(&mut buffered).or_else(distill) {
        Ok(exif) => Ok(facts_from_exif(&exif)),
        Err(exif::Error::NotFound(_)) => Ok(MediaFacts::default()),
        Err(error) => Err(exif_error(error)),
    }
}

/// An EXIF block parsed, keeping whatever a damaged block still holds.
fn parse_block(block: Vec<u8>) -> Result<Option<Exif>, ReadError> {
    let mut parser = exif::Reader::new();
    parser.continue_on_error(true);
    match parser.read_raw(block).or_else(distill) {
        Ok(exif) => Ok(Some(exif)),
        Err(exif::Error::NotFound(_)) => Ok(None),
        Err(error) => Err(exif_error(error)),
    }
}

/// A partial parse's fields, its errors logged.
fn distill(error: exif::Error) -> Result<Exif, exif::Error> {
    error.distill_partial_result(|errors| {
        log::debug!(target: "cmdr_media_meta::photo", "EXIF read partly: {errors:?}");
    })
}

/// `kamadak-exif`'s error as ours, keeping a refusal the byte source raised.
fn exif_error(error: exif::Error) -> ReadError {
    match error {
        exif::Error::Io(io) => ReadError::from_io(io),
        other => ReadError::Io(other.to_string()),
    }
}

/// The fields one parsed EXIF holds.
fn facts_from_exif(exif: &Exif) -> MediaFacts {
    let field = |tag: Tag| exif.get_field(tag, In::PRIMARY).map(|f| &f.value);
    let text = |tag: Tag| match field(tag)? {
        Value::Ascii(strings) => strings
            .iter()
            .find(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned()),
        _ => None,
    };
    let uint = |tag: Tag| field(tag)?.get_uint(0).map(i64::from).filter(|n| *n > 0);
    let date = |tag: Tag| match field(tag)? {
        Value::Ascii(strings) => parse_date(strings.first()?),
        _ => None,
    };
    let focal_mm = match field(Tag::FocalLength) {
        Some(Value::Rational(values)) => values
            .first()
            .map(|r| r.to_f64())
            .filter(|mm| mm.is_finite() && *mm > 0.0 && *mm < 1e6)
            .map(|mm| mm.round() as i64),
        _ => None,
    };
    // IFD0's size is the photo's unless it's marked a preview (DNG, NEF).
    let full_size_ifd0 = field(NEW_SUBFILE_TYPE)
        .and_then(|v| v.get_uint(0))
        .is_none_or(|t| t == 0);
    let ifd0_size = |tag: Tag| if full_size_ifd0 { uint(tag) } else { None };
    MediaFacts {
        taken: date(Tag::DateTimeOriginal)
            .or_else(|| date(Tag::DateTimeDigitized))
            .or_else(|| date(Tag::DateTime)),
        make: text(Tag::Make),
        model: text(Tag::Model),
        lens: text(Tag::LensModel),
        iso: uint(Tag::PhotographicSensitivity),
        focal_mm,
        width: uint(Tag::PixelXDimension).or_else(|| ifd0_size(Tag::ImageWidth)),
        height: uint(Tag::PixelYDimension).or_else(|| ifd0_size(Tag::ImageLength)),
        orientation: uint(Tag::Orientation).filter(|o| (1..=8).contains(o)),
        ..MediaFacts::default()
    }
}

/// EXIF's `2026:07:14 09:05:30`, the camera's wall-clock time. A blank or
/// impossible date (`0000:00:00`) is absent.
fn parse_date(ascii: &[u8]) -> Option<DateTime> {
    let parsed = exif::DateTime::from_ascii(ascii).ok()?;
    DateTime::new(
        i16::try_from(parsed.year).ok()?,
        i8::try_from(parsed.month).ok()?,
        i8::try_from(parsed.day).ok()?,
        i8::try_from(parsed.hour).ok()?,
        i8::try_from(parsed.minute).ok()?,
        i8::try_from(parsed.second.min(59)).ok()?,
        0,
    )
    .ok()
}
