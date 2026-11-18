//! The lossless spelling of a native name that isn't valid Unicode text.
//!
//! An app path is UTF-8 text, and a native name may not be: a Linux disk, SFTP, and NFS
//! name in raw bytes, a Windows disk (and SMB) in UTF-16 units that may hold an unpaired
//! surrogate. Each unit the decoder rejects becomes ONE plane-16 private-use code point, in
//! one of two disjoint alphabets:
//!
//! - **Byte escapes**, U+10FE00 + the byte (U+10FE80–U+10FEFF), from a byte-native
//!   namer ([`Namer::Bytes`]). A rejected byte is always `0x80`–`0xFF`.
//! - **Unit escapes**, U+10E800 + (unit − 0xD800) (U+10E800–U+10EFFF), from a UTF-16
//!   namer ([`Namer::Units`]): an unpaired surrogate.
//!
//! Every escape is a Unicode scalar value, so the escaped text is a valid Rust `String`,
//! JSON, SQLite `TEXT`, and JS string. Private use has no case mapping and no
//! decomposition, so folding and normalization leave an escape alone.
//!
//! **One spelling per native name.** A code point of either alphabet that a native name
//! holds as VALID text is escaped through its own units too (four byte escapes, or two
//! unit escapes), so decoding gives back exactly what the disk holds. Text whose escapes
//! decode to units the decoder would have accepted isn't canonical: [`canonical_name`]
//! turns it into the plain text, and a name mixing both alphabets is refused. Together
//! that makes decoding injective over canonical text: equal native names have equal app
//! spellings, and the reverse.
//!
//! What a person sees: every escape as U+FFFD ([`shown_name`]), exactly what the lossy
//! spelling showed before. The frontend's twin is `apps/desktop/src/lib/path/escape.ts`.

use std::borrow::Cow;

/// The first byte escape's base: byte `b` is `BYTE_BASE + b`.
const BYTE_BASE: u32 = 0x10_FE00;
/// The byte-escape range: the bytes a UTF-8 decoder can reject, `0x80`–`0xFF`.
const BYTE_ESCAPES: std::ops::RangeInclusive<u32> = 0x10_FE80..=0x10_FEFF;
/// The first unit escape: surrogate `u` is `UNIT_BASE + (u - 0xD800)`.
const UNIT_BASE: u32 = 0x10_E800;
/// The unit-escape range: one code point per surrogate, U+D800–U+DFFF.
const UNIT_ESCAPES: std::ops::RangeInclusive<u32> = 0x10_E800..=0x10_EFFF;

/// How a native namer spells its names, which picks the escape alphabet for a unit its
/// decoder rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namer {
    /// Raw bytes, usually UTF-8: a Unix host's disk, SFTP, NFS. Byte escapes.
    Bytes,
    /// UTF-16 units: a Windows host's disk, SMB. Unit escapes.
    Units,
}

impl Namer {
    /// How this host's own disks name things.
    pub const HOST: Namer = super::platform::HOST_NAMER;
}

/// The byte a byte escape stands for.
pub(super) fn byte_of(c: char) -> Option<u8> {
    let c = u32::from(c);
    BYTE_ESCAPES.contains(&c).then(|| (c - BYTE_BASE) as u8)
}

/// The surrogate a unit escape stands for.
pub(super) fn unit_of(c: char) -> Option<u16> {
    let c = u32::from(c);
    UNIT_ESCAPES.contains(&c).then(|| (c - UNIT_BASE + 0xD800) as u16)
}

/// Whether `c` is a code point of either escape alphabet.
pub fn is_escape(c: char) -> bool {
    let c = u32::from(c);
    BYTE_ESCAPES.contains(&c) || UNIT_ESCAPES.contains(&c)
}

fn byte_escape(b: u8) -> char {
    char::from_u32(BYTE_BASE + u32::from(b)).expect("plane 16 private use is a scalar value")
}

fn unit_escape(u: u16) -> char {
    char::from_u32(UNIT_BASE + u32::from(u) - 0xD800).expect("plane 16 private use is a scalar value")
}

/// The units a unit escape stands for, or the literal code point's own UTF-16 units.
fn push_escaped_char(c: char, namer: Namer, out: &mut String) {
    match namer {
        Namer::Bytes => {
            let mut buf = [0u8; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                out.push(byte_escape(b));
            }
        }
        Namer::Units => {
            let mut buf = [0u16; 2];
            for &u in c.encode_utf16(&mut buf).iter() {
                out.push(unit_escape(u));
            }
        }
    }
}

/// Appends `text` with every escape-range code point spelled as LITERAL text: escaped
/// through its own units, the way [`Namer`] would store it. Typed text goes through this,
/// so a person can't type or paste a live escape.
pub(super) fn push_literal(text: &str, namer: Namer, out: &mut String) {
    for c in text.chars() {
        if is_escape(c) {
            push_escaped_char(c, namer, out);
        } else {
            out.push(c);
        }
    }
}

/// `text` with every escape-range code point spelled as literal text (see [`push_literal`]),
/// borrowed when it holds none.
pub(super) fn literal(text: &str, namer: Namer) -> Cow<'_, str> {
    if !text.chars().any(is_escape) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    push_literal(text, namer, &mut out);
    Cow::Owned(out)
}

/// Appends the app spelling of native `bytes`: valid UTF-8 as text, each rejected byte as a
/// byte escape, and each escape-range code point the bytes spell as four byte escapes.
pub(super) fn push_bytes(bytes: &[u8], out: &mut String) {
    for chunk in bytes.utf8_chunks() {
        push_literal(chunk.valid(), Namer::Bytes, out);
        for &b in chunk.invalid() {
            out.push(byte_escape(b));
        }
    }
}

/// The app spelling of native `bytes`, borrowed when they're valid UTF-8 holding no
/// escape-range code point (nearly every name).
pub fn escape_bytes(bytes: &[u8]) -> Cow<'_, str> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return literal(text, Namer::Bytes);
    }
    let mut out = String::with_capacity(bytes.len() + 8);
    push_bytes(bytes, &mut out);
    Cow::Owned(out)
}

/// Appends the app spelling of native UTF-16 `units`: valid pairs and BMP units as text,
/// each unpaired surrogate as a unit escape, and each escape-range code point as two unit
/// escapes.
pub(super) fn push_units(units: &[u16], out: &mut String) {
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(c) if is_escape(c) => push_escaped_char(c, Namer::Units, out),
            Ok(c) => out.push(c),
            Err(lone) => out.push(unit_escape(lone.unpaired_surrogate())),
        }
    }
}

/// The app spelling of native UTF-16 `units`.
pub fn escape_units(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len());
    push_units(units, &mut out);
    out
}

/// A native-name conversion that can't happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeRefusal {
    /// The text holds an escape of the OTHER alphabet: a raw byte can't become a UTF-16
    /// unit, nor an unpaired surrogate a byte.
    Foreign,
    /// The text holds escapes of both alphabets, which no namer makes.
    Mixed,
}

/// The native bytes `text` spells: each byte escape as its byte, other text as UTF-8.
/// Borrowed when `text` holds no escape.
pub fn unescape_bytes(text: &str) -> Result<Cow<'_, [u8]>, EscapeRefusal> {
    if !text.chars().any(is_escape) {
        return Ok(Cow::Borrowed(text.as_bytes()));
    }
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        if let Some(b) = byte_of(c) {
            out.push(b);
        } else if unit_of(c).is_some() {
            return Err(EscapeRefusal::Foreign);
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    Ok(Cow::Owned(out))
}

/// Appends the native UTF-16 units `text` spells: each unit escape as its surrogate, other
/// text as UTF-16.
pub(super) fn push_unescaped_units(text: &str, out: &mut Vec<u16>) -> Result<(), EscapeRefusal> {
    for c in text.chars() {
        if let Some(u) = unit_of(c) {
            out.push(u);
        } else if byte_of(c).is_some() {
            return Err(EscapeRefusal::Foreign);
        } else {
            let mut buf = [0u16; 2];
            out.extend_from_slice(c.encode_utf16(&mut buf));
        }
    }
    Ok(())
}

/// The native UTF-16 units `text` spells.
pub fn unescape_units(text: &str) -> Result<Vec<u16>, EscapeRefusal> {
    let mut out = Vec::with_capacity(text.len());
    push_unescaped_units(text, &mut out)?;
    Ok(out)
}

/// The one canonical spelling of a backend-spelled name (or any text): `escape(unescape(t))`
/// through the alphabet its escapes use. Borrowed when `text` already is canonical, which is
/// every name holding no escape.
///
/// `[U+10FEC3, U+10FEA9]` (the bytes of `é`) becomes `é`; a lone byte escape stays. Text
/// mixing both alphabets is [`EscapeRefusal::Mixed`].
pub fn canonical_name(text: &str) -> Result<Cow<'_, str>, EscapeRefusal> {
    let (mut bytes, mut units) = (false, false);
    for c in text.chars() {
        bytes |= byte_of(c).is_some();
        units |= unit_of(c).is_some();
    }
    let canonical = match (bytes, units) {
        (false, false) => return Ok(Cow::Borrowed(text)),
        (true, true) => return Err(EscapeRefusal::Mixed),
        (true, false) => {
            let native = unescape_bytes(text)?;
            let mut out = String::with_capacity(text.len());
            push_bytes(&native, &mut out);
            out
        }
        (false, true) => escape_units(&unescape_units(text)?),
    };
    Ok(if canonical == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(canonical)
    })
}

/// `text` as a person reads it: every escape as U+FFFD, the replacement character the lossy
/// spelling always showed. Any text, since a plane-16 escape never means anything in prose.
/// Borrowed when there's none.
pub fn shown_name(text: &str) -> Cow<'_, str> {
    if !text.chars().any(is_escape) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .map(|c| if is_escape(c) { char::REPLACEMENT_CHARACTER } else { c })
            .collect(),
    )
}

/// How long `name` is natively, in its namer's unit: one per byte escape or unit escape,
/// the encoded length of everything else. What a 255-byte or 255-unit name limit measures.
pub fn native_name_length(name: &str, namer: Namer) -> usize {
    name.chars()
        .map(|c| {
            if byte_of(c).is_some() || unit_of(c).is_some() {
                1
            } else {
                match namer {
                    Namer::Bytes => c.len_utf8(),
                    Namer::Units => c.len_utf16(),
                }
            }
        })
        .sum()
}

/// Whether `text` holds any escape-range code point.
pub(super) fn has_escape(text: &str) -> bool {
    text.chars().any(is_escape)
}
