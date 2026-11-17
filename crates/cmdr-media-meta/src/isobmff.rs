//! MP4 / MOV / 3GP capture dates: a walk over the top-level boxes by header only,
//! into `moov`'s `mvhd` and Apple's `com.apple.quicktime.creationdate` key.

use std::io::{self, Read, Seek};

use jiff::SignedDuration;
use jiff::civil::DateTime;

use crate::bytes::{be_u32, be_u64, file_len, read_exact_at, read_up_to_at, read_vec_at};
use crate::clock::{ToLocal, container_time, epoch, leading_wall_clock};
use crate::types::MediaFacts;

/// The most children one box's walk visits: a real `moov` has a few dozen, so a
/// file claiming more is garbage, and the walk stops instead of looping.
const MAX_CHILDREN: usize = 4096;

/// The most `keys` an Apple `meta` may list before the rest are ignored.
const MAX_KEYS: usize = 256;

/// One box: its type and where its body starts and ends in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Atom {
    /// The four-character type.
    pub kind: [u8; 4],
    /// The first byte after the header.
    pub body: u64,
    /// One past the last byte, never past the parent's end.
    pub end: u64,
}

/// The box whose header starts at `pos`, if a valid one fits before
/// `parent_end`. A size past the parent's end is cut to it (a truncated file
/// still yields what it has); a size too small for its own header ends the walk.
pub(crate) fn atom_at<R: Read + Seek>(reader: &mut R, pos: u64, parent_end: u64) -> io::Result<Option<Atom>> {
    if pos.saturating_add(8) > parent_end {
        return Ok(None);
    }
    let mut header = [0u8; 16];
    if !read_exact_at(reader, pos, &mut header[..8])? {
        return Ok(None);
    }
    let size32 = be_u32(&header, 0).unwrap_or(0);
    let kind = [header[4], header[5], header[6], header[7]];
    let (header_len, end) = match size32 {
        0 => (8, parent_end),
        1 => {
            if pos.saturating_add(16) > parent_end || !read_exact_at(reader, pos + 8, &mut header[8..])? {
                return Ok(None);
            }
            let size = be_u64(&header, 8).unwrap_or(0);
            if size < 16 {
                return Ok(None);
            }
            (16, pos.saturating_add(size))
        }
        size if size < 8 => return Ok(None),
        size => (8, pos.saturating_add(u64::from(size))),
    };
    Ok(Some(Atom {
        kind,
        body: pos + header_len,
        end: end.min(parent_end),
    }))
}

/// The children of the box whose body spans `body..end`, headers only.
pub(crate) fn children<R: Read + Seek>(reader: &mut R, body: u64, end: u64) -> io::Result<Vec<Atom>> {
    let mut found = Vec::new();
    let mut pos = body;
    while found.len() < MAX_CHILDREN {
        let Some(atom) = atom_at(reader, pos, end)? else {
            break;
        };
        found.push(atom);
        pos = atom.end;
    }
    Ok(found)
}

/// The first child of kind `kind` among `body..end`'s children.
pub(crate) fn child<R: Read + Seek>(reader: &mut R, body: u64, end: u64, kind: &[u8; 4]) -> io::Result<Option<Atom>> {
    Ok(children(reader, body, end)?.into_iter().find(|atom| &atom.kind == kind))
}

/// The top-level `moov`, found by walking box headers and seeking past
/// everything else (`mdat` above all). A `moof` first means a fragmented file
/// whose `moov` would have come before it, so the walk stops there.
pub(crate) fn find_moov<R: Read + Seek>(reader: &mut R) -> io::Result<Option<Atom>> {
    let end = file_len(reader)?;
    let mut pos = 0;
    for _ in 0..MAX_CHILDREN {
        let Some(atom) = atom_at(reader, pos, end)? else {
            break;
        };
        match &atom.kind {
            b"moov" => return Ok(Some(atom)),
            b"moof" => break,
            _ => pos = atom.end,
        }
    }
    Ok(None)
}

/// Where a `meta` box's children start. ISO's `meta` is a full box (four bytes
/// of version and flags first); QuickTime's isn't. Version and flags are zero,
/// and a child's size never is, so four zero bytes mean a full box.
pub(crate) fn meta_children_start<R: Read + Seek>(reader: &mut R, meta: Atom) -> io::Result<u64> {
    let mut first = [0u8; 4];
    let full = read_exact_at(reader, meta.body, &mut first)? && first == [0; 4];
    Ok(if full { meta.body + 4 } else { meta.body })
}

/// The capture date, make, and model `reader`'s boxes say. Apple's written
/// creation date wins (it keeps the wall time the phone showed), then `mvhd`'s
/// UTC creation time on this machine's clock.
pub(crate) fn read_video<R: Read + Seek>(reader: &mut R, to_local: ToLocal<'_>) -> io::Result<MediaFacts> {
    let mut facts = MediaFacts::default();
    let Some(moov) = find_moov(reader)? else {
        return Ok(facts);
    };
    let mut mvhd_time = None;
    for atom in children(reader, moov.body, moov.end)? {
        match &atom.kind {
            b"mvhd" => mvhd_time = read_mvhd(reader, atom)?,
            b"meta" => read_apple_meta(reader, atom, &mut facts)?,
            _ => {}
        }
    }
    facts.taken = facts.taken.or_else(|| mvhd_time.map(to_local));
    Ok(facts)
}

/// `mvhd`'s creation time, UTC: seconds since 1904-01-01, 32-bit in version 0
/// and 64-bit in version 1. Zero and implausible times are absent.
fn read_mvhd<R: Read + Seek>(reader: &mut R, mvhd: Atom) -> io::Result<Option<DateTime>> {
    let mut head = [0u8; 12];
    let len = read_up_to_at(reader, mvhd.body, &mut head)?;
    let head = &head[..len];
    let seconds = match head.first() {
        Some(0) => be_u32(head, 4).map(u64::from),
        Some(1) => be_u64(head, 4),
        _ => None,
    };
    Ok(seconds
        .and_then(|s| i64::try_from(s).ok())
        .and_then(|s| container_time(epoch(1904), SignedDuration::from_secs(s))))
}

/// Apple's `moov/meta`: `keys` names each `ilst` item by 1-based index. Fills
/// `taken` from the creation date as written, and `make` and `model`.
fn read_apple_meta<R: Read + Seek>(reader: &mut R, meta: Atom, facts: &mut MediaFacts) -> io::Result<()> {
    let start = meta_children_start(reader, meta)?;
    let mut keys = Vec::new();
    let mut items = Vec::new();
    for atom in children(reader, start, meta.end)? {
        match &atom.kind {
            b"keys" => keys = read_keys(reader, atom)?,
            b"ilst" => items = children(reader, atom.body, atom.end)?,
            _ => {}
        }
    }
    for item in items {
        let index = u32::from_be_bytes(item.kind) as usize;
        let Some(key) = index.checked_sub(1).and_then(|i| keys.get(i)) else {
            continue;
        };
        let field = match key.as_slice() {
            b"com.apple.quicktime.creationdate" => Field::Date,
            b"com.apple.quicktime.make" => Field::Make,
            b"com.apple.quicktime.model" => Field::Model,
            _ => continue,
        };
        let Some(text) = read_utf8_data(reader, item)? else {
            continue;
        };
        match field {
            Field::Date => facts.taken = facts.taken.or_else(|| parse_apple_date(&text)),
            Field::Make => facts.make = facts.make.take().or(Some(text)),
            Field::Model => facts.model = facts.model.take().or(Some(text)),
        }
    }
    Ok(())
}

/// The Apple keys this reader takes.
enum Field {
    Date,
    Make,
    Model,
}

/// `keys`: a full box with a count, then `size, namespace, name` entries.
fn read_keys<R: Read + Seek>(reader: &mut R, keys: Atom) -> io::Result<Vec<Vec<u8>>> {
    let mut found = Vec::new();
    let mut pos = keys.body + 8;
    while found.len() < MAX_KEYS {
        let Some(entry) = atom_at(reader, pos, keys.end)? else {
            break;
        };
        found.push(read_vec_at(reader, entry.body, entry.end - entry.body)?.unwrap_or_default());
        pos = entry.end;
    }
    Ok(found)
}

/// An `ilst` item's UTF-8 `data` (type 1): `data` box, type, locale, then the
/// text.
fn read_utf8_data<R: Read + Seek>(reader: &mut R, item: Atom) -> io::Result<Option<String>> {
    let Some(data) = child(reader, item.body, item.end, b"data")? else {
        return Ok(None);
    };
    let mut type_and_locale = [0u8; 8];
    if !read_exact_at(reader, data.body, &mut type_and_locale)? || be_u32(&type_and_locale, 0) != Some(1) {
        return Ok(None);
    }
    let text_start = data.body + 8;
    let bytes = read_vec_at(reader, text_start, data.end.saturating_sub(text_start))?;
    Ok(bytes.and_then(|b| String::from_utf8(b).ok()))
}

/// `2026-07-14T09:05:30+0200` → 09:05:30 on that day: the wall time the phone
/// showed, like EXIF's. The offset is ignored.
fn parse_apple_date(text: &str) -> Option<DateTime> {
    leading_wall_clock(text.trim(), "%Y-%m-%dT%H:%M:%S")
}
