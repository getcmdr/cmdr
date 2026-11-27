//! App-path TEXT: the spelling a path is stored and sent as, and the one door that reads it
//! back. A pair, so a value written out and read in is the value it was, for any name.
//!
//! While escapes aren't live ([`super::ESCAPES_LIVE`]) every door that reads stored or wire
//! text reads an escape-range code point in it as LITERAL ([`AppPathBuf::from_literal_store`]),
//! and the text a pane, the index, and a store hold is the name's own text. A value's own
//! spelling ([`AppPath::as_str`]) is not that text for a name holding an escape-range code
//! point: [`AppPathBuf::from_native`] (and every literal door) spells `caf\u{10FEFF}` as four
//! byte escapes, and read back as literal text those turn into sixteen, a path that names no
//! file (#1524: the AI-suggestions preflight called such a file gone). [`AppPath::to_text`]
//! answers the literal text instead, the exact inverse of the door, and refuses, typed, a
//! value no literal text spells (a name that isn't Unicode text), ❌ never a look-alike.
//!
//! Once escapes are live both sides are the value's own spelling and every name round-trips.

use std::borrow::Cow;

use super::escape::{self, Namer};
use super::{AppPath, AppPathBuf, NotAnAppPath};

impl AppPath {
    /// The text this path is stored and sent as, for [`AppPathBuf::from_text`] to read back:
    /// the value's own spelling once escapes are live; until then the name's literal text.
    /// Borrowed when the path holds no escape (nearly every path).
    ///
    /// # Errors
    ///
    /// [`NotAnAppPath::NotUnicode`] while escapes aren't live, for a path whose name isn't
    /// Unicode text: no literal text names it, as [`AppPathBuf::from_native_unicode`] refuses it.
    pub fn to_text(&self) -> Result<Cow<'_, str>, NotAnAppPath> {
        text_in(self, super::ESCAPES_LIVE)
    }
}

impl AppPathBuf {
    /// Stored or wire `text` ([`AppPath::to_text`]'s) as the app path it spells, with this
    /// host's flavor: [`Self::from_wire`] once escapes are live, until then
    /// [`Self::from_literal_store`] through this host's units.
    ///
    /// # Errors
    ///
    /// As [`Self::from_wire`].
    pub fn from_text(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
        from_text_in(text, super::ESCAPES_LIVE)
    }
}

/// [`AppPath::to_text`] with escapes live or not, so both behaviors are tested today.
pub(super) fn text_in(path: &AppPath, live: bool) -> Result<Cow<'_, str>, NotAnAppPath> {
    let spelled = path.as_str();
    if live || !escape::has_escape(spelled) {
        return Ok(Cow::Borrowed(spelled));
    }
    // Decoded through this host's alphabet, the one `from_literal_store` escapes through, and
    // only where the escapes are exactly the literal spelling of what they decode to: anything
    // else (a raw byte or a lone surrogate, the other alphabet) is a name no text spells yet.
    let decoded = match Namer::HOST {
        Namer::Bytes => escape::unescape_bytes(spelled)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes.into_owned()).ok()),
        Namer::Units => escape::unescape_units(spelled)
            .ok()
            .and_then(|units| String::from_utf16(&units).ok()),
    }
    .ok_or(NotAnAppPath::NotUnicode)?;
    if escape::literal(&decoded, Namer::HOST) != spelled {
        return Err(NotAnAppPath::NotUnicode);
    }
    Ok(Cow::Owned(decoded))
}

/// [`AppPathBuf::from_text`] with escapes live or not.
pub(super) fn from_text_in(text: &str, live: bool) -> Result<AppPathBuf, NotAnAppPath> {
    if live {
        AppPathBuf::from_wire(text)
    } else {
        AppPathBuf::from_literal_store(text, Namer::HOST)
    }
}

#[cfg(test)]
#[path = "text_test.rs"]
mod tests;
