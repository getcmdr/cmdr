//! MKV / WebM / MKA: an EBML walk guided by `SeekHead` to `Info` (`DateUTC`) and
//! `Tags` (`DATE_RECORDED` and the simple text tags). Clusters are skipped by size.

use std::io::{self, Read, Seek};

use jiff::SignedDuration;
use jiff::civil::DateTime;

use crate::bytes::{file_len, read_up_to_at, read_vec_at};
use crate::clock::{ToLocal, container_time, epoch, leading_wall_clock};
use crate::types::MediaFacts;

const EBML_HEADER: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const SEEK: u32 = 0x4DBB;
const SEEK_ID: u32 = 0x53AB;
const SEEK_POSITION: u32 = 0x53AC;
const INFO: u32 = 0x1549_A966;
const DATE_UTC: u32 = 0x4461;
const CLUSTER: u32 = 0x1F43_B675;
const TAGS: u32 = 0x1254_C367;
const TAG: u32 = 0x7373;
const TARGETS: u32 = 0x63C0;
const TARGET_TYPE_VALUE: u32 = 0x68CA;
/// `TagTrackUID`, `TagEditionUID`, `TagChapterUID`, `TagAttachmentUID`: a tag
/// aimed at one part of the file, not the file.
const TARGET_UIDS: [u32; 4] = [0x63C5, 0x63C9, 0x63C4, 0x63C6];
const SIMPLE_TAG: u32 = 0x67C8;
const TAG_NAME: u32 = 0x45A3;
const TAG_STRING: u32 = 0x4487;

/// The most elements one level's walk visits before it gives up on the file.
const MAX_ELEMENTS: usize = 4096;
/// The most `SeekHead` jumps one read follows: a real file needs two or three.
const MAX_JUMPS: usize = 16;
/// The most simple tags one read keeps.
const MAX_SIMPLE_TAGS: usize = 1024;
/// `TargetTypeValue` when `Targets` doesn't say: 50, the album or movie.
const DEFAULT_LEVEL: u64 = 50;

/// One element: its id (length marker kept, as the spec writes ids), where its
/// data starts, and where it ends (`None` for an unknown size).
#[derive(Debug, Clone, Copy)]
struct Element {
    id: u32,
    data: u64,
    end: Option<u64>,
}

/// A simple tag of a tag aimed at the whole file.
struct SimpleTag {
    level: u64,
    name: String,
    value: String,
}

/// What the walk collects before it's turned into facts.
#[derive(Default)]
struct Found {
    date_utc: Option<DateTime>,
    tags: Vec<SimpleTag>,
}

/// The date and tags `reader`'s elements say. `DATE_RECORDED` keeps the wall
/// time it was written in; `DateUTC` (often the muxing time) is the fallback,
/// on this machine's clock.
pub(crate) fn read_matroska<R: Read + Seek>(reader: &mut R, to_local: ToLocal<'_>) -> io::Result<MediaFacts> {
    let file_end = file_len(reader)?;
    let Some(segment) = find_segment(reader, file_end)? else {
        return Ok(MediaFacts::default());
    };
    // A live recording leaves the segment's size unknown: it runs to the end.
    let segment_end = segment.end.unwrap_or(file_end);
    let mut found = Found::default();
    let mut jumps: Vec<(u32, u64)> = Vec::new();
    let mut visited: Vec<u64> = Vec::new();

    // The elements before the first cluster, in order; a cluster (or anything of
    // unknown size) ends it, and the seek head's targets cover the rest.
    let mut pos = segment.data;
    for _ in 0..MAX_ELEMENTS {
        let Some(element) = element_at(reader, pos, segment_end)? else {
            break;
        };
        if element.id == CLUSTER {
            break;
        }
        visited.push(pos);
        visit(reader, element, segment.data, &mut found, &mut jumps)?;
        let Some(end) = element.end else {
            break;
        };
        pos = end;
    }

    let mut next = 0;
    while next < jumps.len().min(MAX_JUMPS) {
        let (id, at) = jumps[next];
        next += 1;
        if visited.contains(&at) {
            continue;
        }
        visited.push(at);
        if let Some(element) = element_at(reader, at, segment_end)?
            && element.id == id
        {
            visit(reader, element, segment.data, &mut found, &mut jumps)?;
        }
    }
    Ok(facts_from(found, to_local))
}

/// The `Segment` after the EBML header, or `None` when this isn't EBML.
fn find_segment<R: Read + Seek>(reader: &mut R, file_end: u64) -> io::Result<Option<Element>> {
    let Some(header) = element_at(reader, 0, file_end)? else {
        return Ok(None);
    };
    let (EBML_HEADER, Some(mut pos)) = (header.id, header.end) else {
        return Ok(None);
    };
    // Only `Void` or `CRC-32` may sit between them; a few is plenty.
    for _ in 0..16 {
        let Some(element) = element_at(reader, pos, file_end)? else {
            break;
        };
        if element.id == SEGMENT {
            return Ok(Some(element));
        }
        let Some(end) = element.end else {
            break;
        };
        pos = end;
    }
    Ok(None)
}

/// Reads one top-level element of the segment the walk cares about.
fn visit<R: Read + Seek>(
    reader: &mut R,
    element: Element,
    segment_data: u64,
    found: &mut Found,
    jumps: &mut Vec<(u32, u64)>,
) -> io::Result<()> {
    match element.id {
        SEEK_HEAD => read_seek_head(reader, element, segment_data, jumps),
        INFO => {
            for child in children(reader, element)? {
                if child.id == DATE_UTC
                    && let Some(nanos) = read_int(reader, child)?
                {
                    found.date_utc = container_time(epoch(2001), SignedDuration::from_nanos(nanos));
                }
            }
            Ok(())
        }
        TAGS => {
            for tag in children(reader, element)? {
                if tag.id == TAG {
                    read_tag(reader, tag, &mut found.tags)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `SeekHead`'s entries for `Info`, `Tags`, and further seek heads, as file
/// offsets (a position counts from the segment's data start).
fn read_seek_head<R: Read + Seek>(
    reader: &mut R,
    head: Element,
    segment_data: u64,
    jumps: &mut Vec<(u32, u64)>,
) -> io::Result<()> {
    for seek in children(reader, head)? {
        if seek.id != SEEK {
            continue;
        }
        let (mut id, mut position) = (None, None);
        for field in children(reader, seek)? {
            match field.id {
                SEEK_ID => id = read_uint(reader, field)?.and_then(|v| u32::try_from(v).ok()),
                SEEK_POSITION => position = read_uint(reader, field)?,
                _ => {}
            }
        }
        if let (Some(id @ (INFO | TAGS | SEEK_HEAD)), Some(position)) = (id, position)
            && let Some(at) = segment_data.checked_add(position)
            && jumps.len() < MAX_JUMPS
        {
            jumps.push((id, at));
        }
    }
    Ok(())
}

/// One `Tag`'s simple tags, unless its `Targets` aim it at a track, an
/// edition, a chapter, or an attachment.
fn read_tag<R: Read + Seek>(reader: &mut R, tag: Element, into: &mut Vec<SimpleTag>) -> io::Result<()> {
    let mut level = DEFAULT_LEVEL;
    let mut pairs = Vec::new();
    for child in children(reader, tag)? {
        match child.id {
            TARGETS => {
                for target in children(reader, child)? {
                    let value = read_uint(reader, target)?;
                    if target.id == TARGET_TYPE_VALUE {
                        level = value.unwrap_or(DEFAULT_LEVEL);
                    } else if TARGET_UIDS.contains(&target.id) && value.is_some_and(|uid| uid != 0) {
                        return Ok(());
                    }
                }
            }
            SIMPLE_TAG => {
                let (mut name, mut value) = (None, None);
                for field in children(reader, child)? {
                    match field.id {
                        TAG_NAME => name = read_string(reader, field)?,
                        TAG_STRING => value = read_string(reader, field)?,
                        _ => {}
                    }
                }
                if let (Some(name), Some(value)) = (name, value) {
                    pairs.push((name, value));
                }
            }
            _ => {}
        }
    }
    let room = MAX_SIMPLE_TAGS.saturating_sub(into.len());
    into.extend(
        pairs
            .into_iter()
            .take(room)
            .map(|(name, value)| SimpleTag { level, name, value }),
    );
    Ok(())
}

/// The facts the collected elements make. Matroska nests a track inside an album
/// by level (30 track, 50 album), so the lowest level's `TITLE` is the title and
/// an album-level one beside it is the album.
fn facts_from(found: Found, to_local: ToLocal<'_>) -> MediaFacts {
    let tags = &found.tags;
    let lowest = |name: &str| tags.iter().filter(|tag| tag.name == name).min_by_key(|tag| tag.level);
    let album_level = |name: &str| {
        let lowest = lowest(name)?;
        tags.iter()
            .filter(|tag| tag.name == name && tag.level >= DEFAULT_LEVEL && tag.level > lowest.level)
            .min_by_key(|tag| tag.level)
            .map(|tag| tag.value.clone())
    };
    let any = |name: &str| tags.iter().find(|tag| tag.name == name).map(|tag| tag.value.clone());
    let value = |name: &str| lowest(name).map(|tag| tag.value.clone());

    let recorded = tags
        .iter()
        .filter(|tag| tag.name == "DATE_RECORDED")
        .find_map(|tag| parse_date(&tag.value));
    let track = lowest("PART_NUMBER")
        .filter(|tag| tag.level < DEFAULT_LEVEL)
        .and_then(|tag| leading_number(&tag.value));
    let year = any("DATE_RELEASED")
        .or_else(|| any("DATE"))
        .and_then(|date| leading_year(&date));
    MediaFacts {
        taken: recorded.or_else(|| found.date_utc.map(to_local)),
        title: value("TITLE"),
        artist: value("ARTIST"),
        album_artist: any("ALBUM_ARTIST").or_else(|| album_level("ARTIST")),
        album: any("ALBUM").or_else(|| album_level("TITLE")),
        genre: value("GENRE"),
        comment: value("COMMENT"),
        track,
        year,
        ..MediaFacts::default()
    }
}

/// `YYYY-MM-DD hh:mm:ss[.mss]` (or with a `T`), as Matroska writes dates.
fn parse_date(text: &str) -> Option<DateTime> {
    let text = text.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"]
        .iter()
        .find_map(|format| leading_wall_clock(text, format))
}

/// The whole number `text` starts with (`3`, `3/12`).
fn leading_number(text: &str) -> Option<i64> {
    let digits: String = text.trim().chars().take_while(char::is_ascii_digit).take(9).collect();
    digits.parse().ok()
}

/// The four-digit year `text` starts with.
fn leading_year(text: &str) -> Option<i64> {
    let year = text.trim().get(..4)?;
    year.chars()
        .all(|c| c.is_ascii_digit())
        .then(|| year.parse().ok())
        .flatten()
}

/// The element whose header starts at `pos`, if a valid one fits before
/// `parent_end`. A size past the parent's end is cut to it.
fn element_at<R: Read + Seek>(reader: &mut R, pos: u64, parent_end: u64) -> io::Result<Option<Element>> {
    if pos >= parent_end {
        return Ok(None);
    }
    let mut header = [0u8; 12];
    let len = read_up_to_at(reader, pos, &mut header)?;
    let header = &header[..len];
    let Some((_, id_len)) = varint(header, 0, 4) else {
        return Ok(None);
    };
    let Some((size, size_len)) = varint(header, id_len, 8) else {
        return Ok(None);
    };
    // Ids keep their marker bit; sizes drop it, and all ones means "unknown".
    let id = header[..id_len].iter().fold(0u32, |acc, b| (acc << 8) | u32::from(*b));
    let unknown = size == (1u64 << (7 * size_len)) - 1;
    let data = pos + (id_len + size_len) as u64;
    if data > parent_end {
        return Ok(None);
    }
    let end = if unknown {
        None
    } else {
        Some(data.saturating_add(size).min(parent_end))
    };
    Ok(Some(Element { id, data, end }))
}

/// An EBML variable-length integer at `at`, at most `max_len` bytes: its value
/// without the length marker, and its length.
fn varint(bytes: &[u8], at: usize, max_len: usize) -> Option<(u64, usize)> {
    let first = *bytes.get(at)?;
    let len = first.leading_zeros() as usize + 1;
    if len > max_len {
        return None;
    }
    let rest = bytes.get(at + 1..at + len)?;
    let marker_cleared = u64::from(first) & (0xFF >> len);
    let value = rest.iter().fold(marker_cleared, |acc, b| (acc << 8) | u64::from(*b));
    Some((value, len))
}

/// A master element's children, headers only. One of unknown size ends the list.
fn children<R: Read + Seek>(reader: &mut R, parent: Element) -> io::Result<Vec<Element>> {
    let Some(end) = parent.end else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    let mut pos = parent.data;
    while found.len() < MAX_ELEMENTS {
        let Some(element) = element_at(reader, pos, end)? else {
            break;
        };
        found.push(element);
        let Some(next) = element.end else {
            break;
        };
        pos = next;
    }
    Ok(found)
}

/// An element's data, when it's small enough to hold.
fn read_data<R: Read + Seek>(reader: &mut R, element: Element) -> io::Result<Option<Vec<u8>>> {
    let Some(end) = element.end else {
        return Ok(None);
    };
    read_vec_at(reader, element.data, end - element.data)
}

/// An unsigned integer element (zero to eight bytes, big-endian).
fn read_uint<R: Read + Seek>(reader: &mut R, element: Element) -> io::Result<Option<u64>> {
    if element.end.is_none_or(|end| end - element.data > 8) {
        return Ok(None);
    }
    Ok(read_data(reader, element)?.map(|bytes| bytes.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b))))
}

/// A signed integer element (eight bytes for a date).
fn read_int<R: Read + Seek>(reader: &mut R, element: Element) -> io::Result<Option<i64>> {
    let bytes = read_data(reader, element)?;
    Ok(bytes
        .and_then(|b| <[u8; 8]>::try_from(b.as_slice()).ok())
        .map(i64::from_be_bytes))
}

/// A UTF-8 string element; invalid UTF-8 is no value.
fn read_string<R: Read + Seek>(reader: &mut R, element: Element) -> io::Result<Option<String>> {
    Ok(read_data(reader, element)?.and_then(|bytes| String::from_utf8(bytes).ok()))
}
