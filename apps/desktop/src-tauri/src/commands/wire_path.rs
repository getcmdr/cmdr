//! The command edge's door for a path the frontend sends: wire text is read as an app path
//! through `AppPathBuf::from_wire`, so a trailing `/` (and a repeated `/`, a `.` name) is
//! dropped and a `..` name is refused (`docs/specs/app-path-plan.md` § "Normalization").
//!
//! - **Why the trailing `/`**: POSIX resolves `x/link/` THROUGH a symlink, so a stat or a
//!   delete of it named the link's target. Without it, the path names what its last name
//!   says.
//! - **Why the `..` refusal**: the OS reads `x/link/../y` as the link TARGET's sibling, and
//!   a lexical fold names a different file. Only a volume answers it, and text a person or an
//!   agent typed goes through the typed-path resolver first, so a `..` reaching a command
//!   from a pane, a row, or a store is a sender bug: refused typed and logged as one
//!   ([`refused`]).
//! - **An engine start's sources** ([`checked`]) are checked here and handed on as sent:
//!   every write start drops a trailing `/` itself (`crates/cmdr-operations/src/wire_paths.rs`),
//!   so the bytes reach the engine as they always did. Text that's no app path (relative,
//!   empty) keeps today's answer.
//! - **A person-typed destination** (the transfer dialog's box) anchors on its volume's root
//!   ([`anchored`]), and a `..` in it is no sender bug: its probe can't tell, and the transfer
//!   keeps today's OS answer until the typed-path resolver takes that text.
//!
//! While escapes aren't live ([`ESCAPES_LIVE`]), an escape-range code point in wire text is
//! LITERAL (no producer made it), so it parses through `from_literal_store`, and the native
//! path out is byte for byte the text in.

use std::borrow::Cow;
use std::path::PathBuf;

use cmdr_fs::app_path::{AppPath, AppPathBuf, ESCAPES_LIVE, Namer, NotAnAppPath, PathFlavor, is_escape};
use cmdr_fs::error_detail::ErrorDetail;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::{VolumeError, anchor_at_root};
use cmdr_operations::{MutationError, WriteOperationError};

/// A wire path the command edge refused: a `..` name, or (for a volume call) text that's no
/// app path. The caller turns it into its own family's typed error; [`refused`] has already
/// logged it as a bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WirePathRefused {
    /// The text as the frontend sent it, for the error's technical details.
    pub(crate) path: String,
    /// Why it names no place a command acts on.
    pub(crate) why: NotAnAppPath,
}

impl WirePathRefused {
    /// The technical-details line: the path, then why.
    fn detail(&self) -> ErrorDetail {
        ErrorDetail::path_then(self.path.clone(), format!(": {}, which no command acts on", self.why))
    }
}

/// A write start's refusal: the generic I/O failure ("Couldn't delete the file"), whose
/// technical details name the path. A bug-class event has no copy of its own to owe.
impl From<WirePathRefused> for WriteOperationError {
    fn from(refused: WirePathRefused) -> Self {
        WriteOperationError::IoError {
            message: refused.detail(),
            path: refused.path,
        }
    }
}

/// An instant mutation's refusal: the one "something went wrong" fallback, details as above.
impl From<WirePathRefused> for MutationError {
    fn from(refused: WirePathRefused) -> Self {
        MutationError::Unexpected {
            detail: refused.detail(),
        }
    }
}

/// A volume call's refusal: "The volume couldn't finish that", details as above.
impl From<WirePathRefused> for VolumeError {
    fn from(refused: WirePathRefused) -> Self {
        VolumeError::IoError {
            message: refused.detail(),
            raw_os_error: None,
        }
    }
}

/// `text` as an app path: `Some` normalized, `None` for text that's no app path. A `..`
/// name anywhere is refused.
pub(crate) fn app_path(text: &str) -> Result<Option<AppPathBuf>, WirePathRefused> {
    match parsed(text) {
        Ok(path) => Ok(Some(path)),
        Err(_) if has_dot_dot_name(text) => Err(refused(text, NotAnAppPath::DotName)),
        Err(_) => Ok(None),
    }
}

/// An engine start's source, checked: refused for a `..`, else the text as sent (the start
/// drops a trailing `/` itself, byte for byte, so a row's path reaches it unchanged).
pub(crate) fn checked(text: &str) -> Result<PathBuf, WirePathRefused> {
    app_path(text)?;
    Ok(PathBuf::from(text))
}

/// A [`checked`] source as the routing seam reads it (the scan preview's route): the
/// engine's starts take the `PathBuf` back, so this crosses it through the bridge, byte
/// for byte. Goes when the starts take app paths (milestone 10d).
pub(crate) fn checked_as_app_path(source: &std::path::Path) -> AppPathBuf {
    AppPath::legacy_unchecked(source).into_owned()
}

/// [`checked`] over a start's sources, refusing the whole set on the first `..`.
pub(crate) fn checked_all<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<Vec<PathBuf>, WirePathRefused> {
    texts.into_iter().map(checked).collect()
}

/// `text` as the app path a `Volume` method takes, normalized. A POSIX `/x` under the
/// Windows flavor reads as POSIX (a volume-relative path on a phone or a share). Refused
/// for a `..`, and for text that names no place (relative, empty).
pub(crate) fn on_volume(text: &str) -> Result<AppPathBuf, WirePathRefused> {
    read_on_volume(text).map_err(|why| refused(text, why))
}

/// [`on_volume`] without the bug report, for text an AGENT wrote (a stored proposal's
/// destination), where a `..` or a relative path is the agent's, not a sender bug.
pub(crate) fn read_on_volume(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    if has_dot_dot_name(text) {
        return Err(NotAnAppPath::DotName);
    }
    parsed(text).or_else(|_| parsed_posix(text))
}

/// A destination box's text (VOLUME-RELATIVE or absolute, typed by a person) anchored on the
/// volume rooted at `root`, through the typed `anchor_at_root`: `/photos` on a share is
/// `<root>/photos`, and on Windows the root volume's `C:/` takes a POSIX `/Users/x` as
/// `C:/Users/x`. A trailing `/` goes.
///
/// `None`, with no bug report, for text holding a `..`, text that names another volume, and
/// text that's no app path: a person typed it, and the typed-path resolver is what answers a
/// `..` (`docs/specs/app-path-plan.md` § "Typed and pasted `..`"). The caller's probe answers
/// "couldn't tell" for it; the transfer itself still anchors it the `std::path` way
/// (`cmdr_operations::resolve_dest_path`), which leaves the `..` to the OS.
pub(crate) fn anchored(root: &AppPath, text: &str) -> Option<AppPathBuf> {
    if has_dot_dot_name(text) {
        return None;
    }
    // `anchor_at_root` reads app spelling, so text that parses goes in as its literal
    // spelling; other text holding an escape-range code point has none, so stays as typed.
    let spelled: Cow<'_, str> = match parsed(text) {
        Ok(path) => Cow::Owned(path.as_str().to_owned()),
        Err(_) if !ESCAPES_LIVE && text.chars().any(is_escape) => return None,
        Err(_) => Cow::Borrowed(text),
    };
    anchor_at_root(root, &spelled).ok()
}

/// `text` parsed with this host's flavor; its escape-range code points literal while no
/// producer makes a live one.
fn parsed(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    if ESCAPES_LIVE {
        AppPathBuf::from_wire(text)
    } else {
        AppPathBuf::from_literal_store(text, Namer::HOST)
    }
}

/// [`parsed`] for the POSIX flavor, whatever the host: a volume-relative `/x` on Windows.
/// Text holding an escape-range code point has no literal POSIX spelling to read here, so
/// it's refused as one this door doesn't read.
fn parsed_posix(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    if !ESCAPES_LIVE && text.chars().any(is_escape) {
        return Err(NotAnAppPath::WrongFlavor);
    }
    AppPathBuf::parse(text, PathFlavor::Posix)
}

/// Whether one of `text`'s names is `..` (`/`-separated, and `\` too on Windows). `..x`
/// and `x..` are ordinary names.
pub(crate) fn has_dot_dot_name(text: &str) -> bool {
    let windows = PathFlavor::HOST == PathFlavor::Windows;
    text.split(|c| c == '/' || (windows && c == '\\'))
        .any(|name| name == "..")
}

/// The refusal, logged as the bug it is: every sender of these paths builds them from rows
/// or a pane's own path, which never hold a `..`.
fn refused(text: &str, why: NotAnAppPath) -> WirePathRefused {
    crate::log_error!(
        target: "wire_path",
        "a command was sent a path that names no place it acts on ({why}): refused path={:?}",
        LogPrivate::path(text)
    );
    WirePathRefused {
        path: text.to_owned(),
        why,
    }
}

#[cfg(test)]
#[path = "wire_path_test.rs"]
mod wire_path_test;
