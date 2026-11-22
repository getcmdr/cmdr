//! App-path TEXT read as the app path it spells, for any layer: a pane's row, a stored path, a
//! path the frontend sent. The command edge's door (`commands::wire_path`) reads through here,
//! and so do the subsystems below it that get rows as `String`s (the sync-status badges, a
//! favorite's folder), so there's one reading of app text, ❌ never a `Path::new` of it.
//!
//! While escapes aren't live (`ESCAPES_LIVE`), an escape-range code point in the text is
//! LITERAL (no producer made it), so it parses through `from_literal_store`, and the native
//! path out is byte for byte the text in.

use std::path::PathBuf;

use cmdr_fs::app_path::{AppPathBuf, ESCAPES_LIVE, Namer, NotAnAppPath};

/// `text` as an app path, with this host's flavor, normalized (a trailing `/` and `.` names
/// dropped).
///
/// # Errors
///
/// [`NotAnAppPath`]: relative or empty text, a `..` name, the other OS's shape.
pub(crate) fn parse(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    if ESCAPES_LIVE {
        AppPathBuf::from_wire(text)
    } else {
        AppPathBuf::from_literal_store(text, Namer::HOST)
    }
}

/// [`parse`] for the POSIX flavor, whatever the host: a volume-relative `/x` on Windows (a
/// phone's or a share's path). Text holding an escape-range code point has no literal POSIX
/// spelling to read here, so it's refused as one this door doesn't read.
///
/// # Errors
///
/// As [`parse`], and [`NotAnAppPath::WrongFlavor`] for an escape-range code point.
pub(crate) fn parse_posix(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    if !ESCAPES_LIVE && text.chars().any(cmdr_fs::app_path::is_escape) {
        return Err(NotAnAppPath::WrongFlavor);
    }
    AppPathBuf::parse(text, cmdr_fs::app_path::PathFlavor::Posix)
}

/// App text that may name a place on ANY volume (a journal row, a card's place): [`parse`],
/// else [`parse_posix`], so a phone's `/DCIM/x` reads on a Windows host too.
///
/// # Errors
///
/// As [`parse_posix`].
pub(crate) fn parse_on_volume(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    parse(text).or_else(|_| parse_posix(text))
}

/// Whether a ROUTE serves the place `text` names (inside an archive, a repo's `.git` trees, a
/// snapshot), asked of its app path. Text that's no app path routes nowhere.
pub(crate) fn routes_over_its_parent(text: &str) -> bool {
    parse(text).is_ok_and(|path| crate::file_system::volume::manager::app_path_routes_over_its_parent(&path))
}

/// The last name of the app path `text` spells on any volume ([`parse_on_volume`]), `None` at
/// a root and for text that is no app path.
pub(crate) fn last_name(text: &str) -> Option<String> {
    use cmdr_fs::app_path::PathArithmetic;
    parse_on_volume(text).ok()?.file_name().map(str::to_owned)
}

/// The native file `text` names: [`parse`], then `to_native`. `None` for text that's no app
/// path and for an app path this OS's files don't hold (a scheme path).
pub(crate) fn native(text: &str) -> Option<PathBuf> {
    parse(text).ok()?.to_native().ok()
}

/// App text handed, byte for byte, to an engine that still keys and answers by `std::path`
/// TEXT (the git portal's rows, the listing engine's stored spelling, the write engine's
/// pre-checks), so its answer lands back on the same row. The engine reads it lexically and
/// hands it back; ❌ not for a path the OS opens: that's [`native`]. It goes as each engine's
/// entry points take app paths. `commands::wire_path::as_sent_std_path` is this at the command
/// edge.
pub(crate) fn as_std_path_text(text: &str) -> PathBuf {
    PathBuf::from(text)
}

/// [`native`] in the PLAIN form (no `\\?\` on Windows), for code that compares the path with
/// spellings the OS hands out plain (the home folder, a pane's tab, a registered mount) or hands
/// it to a crate that can't take a verbatim path; the verbatim form only where no plain one names
/// the same file. Byte for byte [`native`] on macOS and Linux.
pub(crate) fn native_plain(text: &str) -> Option<PathBuf> {
    let path = parse(text).ok()?;
    path.to_native_plain().or_else(|_| path.to_native()).ok()
}

#[cfg(test)]
#[path = "app_text_test.rs"]
mod tests;
