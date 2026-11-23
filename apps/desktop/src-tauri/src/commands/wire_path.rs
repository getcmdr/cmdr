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
//! - **A write start's paths** ([`start_path`]) are read here as app paths, the type the
//!   engine's starts take; text that's no app path (relative, empty) is refused with the
//!   `..`. The engine's pre-checks and the instant ops outside the starts (the single
//!   trash, the rename pre-flight) still take `std::path` values ([`checked`]), handed on
//!   as sent: each drops a trailing `/` itself (`crates/cmdr-operations/src/wire_paths.rs`).
//! - **A person-typed destination** (the transfer dialog's box) anchors on its volume's root
//!   ([`anchored`]), and a `..` in it is no sender bug: [`destination`] resolves it through the
//!   typed-path resolver on the volume doing the write, so the probes, the conflict scan, and
//!   the start ([`transfer_destination`]) all name the place `ls` names, and the engine never
//!   sees a `..`. Text with no `..` crosses as it always did.
//!
//! While escapes aren't live ([`ESCAPES_LIVE`]), an escape-range code point in wire text is
//! LITERAL (no producer made it), so it parses through `from_literal_store`, and the native
//! path out is byte for byte the text in.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use cmdr_fs::app_path::{AppPath, AppPathBuf, ESCAPES_LIVE, NotAnAppPath, PathFlavor, is_escape, is_scheme_url};
use cmdr_fs::error_detail::ErrorDetail;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::typed_path::{TypedBy, TypedPathRefusal};
use cmdr_fs::volume::{Volume, VolumeError, anchor_at_root};
use cmdr_operations::{MutationError, VolumeScanError, WriteOperationError};

use crate::file_system::volume::manager::get_volume_manager;
use crate::go_to_path::AppHost;

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

/// Why wire text has no path this OS's files and frameworks open ([`native`]). Every caller
/// answers all three alike (nothing to open), so they carry nothing but their name for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoNativePath {
    /// A `..` name, logged as the sender bug it is.
    Refused,
    /// Text that's no app path: relative, or empty.
    NotAnAppPath,
    /// An app path this OS's files don't hold: a scheme path, or the other OS's shape.
    NotLocal,
}

/// Wire text an OS integration acts on (Finder, the shell, a framework, a `std::fs` read),
/// read at the edge as [`app_path`] reads it and crossed down once through `to_native`. A
/// trailing `/` goes, so `x/link/` names the link.
///
/// # Errors
///
/// [`NoNativePath`]: a `..` name, text that's no app path, or an app path with no native
/// form here.
pub(crate) fn native(text: &str) -> Result<PathBuf, NoNativePath> {
    let path = app_path(text)
        .map_err(|_| NoNativePath::Refused)?
        .ok_or(NoNativePath::NotAnAppPath)?;
    path.to_native().map_err(|_| NoNativePath::NotLocal)
}

/// Whether a ROUTE serves the wire path `text` (inside an archive, a repo's `.git` trees, a
/// snapshot), asked of its app path. Text that's no app path, or holds a `..`, routes nowhere:
/// no route serves a place no app path names.
pub(crate) fn routes_over_its_parent(text: &str) -> bool {
    matches!(app_path(text), Ok(Some(path)) if crate::file_system::volume::manager::app_path_routes_over_its_parent(&path))
}

/// A path for an engine entry point that still takes `std::path` (the single trash, the
/// trash-folder lookup, the rename pre-flight), checked: refused for a `..`, else the text as
/// sent (the entry point drops a trailing `/` itself, byte for byte).
pub(crate) fn checked(text: &str) -> Result<PathBuf, WirePathRefused> {
    app_path(text)?;
    Ok(PathBuf::from(text))
}

/// Text a `std::path` API reads LEXICALLY, handed on as sent with no `..` check: the write
/// engine's pre-checks and the transfer box's text (which [`destination`] answers where a
/// `..` matters), the listing engine's stored-spelling lookup, `root_echo`, and the git chip's
/// `repo_root` (native text `RepoInfo` reported, handed back). Those APIs
/// still speak `std::path`, so this is their one crossing from the command edge, byte for
/// byte; it goes as each engine's entry points take app paths (`cmdr-listing`'s with its typed
/// listing locations, the write engine's in milestone 10d). ❌ Not for a path the OS opens: that's [`native`].
pub(crate) fn as_sent_std_path(text: &str) -> PathBuf {
    crate::app_text::as_std_path_text(text)
}

/// A write pre-check's rows (the home-folder guard, the scan preview) as its engine call takes
/// them: [`as_sent_std_path`], `~` expanded on the local volume only (a phone's or a share's
/// path is volume-relative, `commands/CLAUDE.md`).
pub(crate) fn pre_check_rows(rows: &[String], is_local: bool) -> Vec<PathBuf> {
    let as_sent = |row: &String| {
        if is_local {
            as_sent_std_path(&cmdr_fs::tilde::expand_tilde(row))
        } else {
            as_sent_std_path(row)
        }
    };
    rows.iter().map(as_sent).collect()
}

/// A write start's path (copy, move, delete, trash, rename, compress), as the engine's
/// starts take it: the app path [`on_volume`] reads. A trailing `/` goes, so `x/link/`
/// names the link; a `..` is refused, as is text that names no place (relative, empty),
/// which no row or pane sends.
pub(crate) fn start_path(text: &str) -> Result<AppPathBuf, WirePathRefused> {
    on_volume(text)
}

/// [`start_path`] over a start's sources, refusing the whole set on the first refusal.
pub(crate) fn start_paths<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<Vec<AppPathBuf>, WirePathRefused> {
    texts.into_iter().map(start_path).collect()
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
/// `..` ([`destination`], `docs/specs/app-path-plan.md` § "Typed and pasted `..`").
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

/// A write start's destination box text, already resolved ([`transfer_destination`]),
/// anchored on the volume rooted at `root` ([`anchored`]) as the start takes it. A `..`
/// left in it, or text on another volume, is refused as a sender bug: the resolve ran first.
pub(crate) fn start_destination(root: &AppPath, text: &str) -> Result<AppPathBuf, WirePathRefused> {
    anchored(root, text).ok_or_else(|| {
        let why = if has_dot_dot_name(text) {
            NotAnAppPath::DotName
        } else {
            NotAnAppPath::WrongFlavor
        };
        refused(text, why)
    })
}

/// Why a destination box's text names no place on its volume ([`destination`]).
#[derive(Debug)]
pub(crate) enum DestinationRefusal {
    /// No path on this volume: another volume's, no app path at all, or (while escapes
    /// aren't live) a `..` beside an escape-range code point, which has no typed spelling.
    NoAnchor,
    /// The volume couldn't say what a `..` means there (a hung mount, a link loop).
    Unresolved(TypedPathRefusal),
    /// A `..` led off the volume: through a link to another disk, or above a share's root.
    OffVolume,
}

impl DestinationRefusal {
    /// A transfer start's refusal of the box text `path`: a volume that didn't answer is
    /// the interrupted connection it was; anything else names no folder to write into.
    pub(crate) fn into_write_error(self, path: String) -> WriteOperationError {
        match self {
            Self::Unresolved(TypedPathRefusal::Unanswered(
                VolumeError::ConnectionTimeout(_)
                | VolumeError::NotConnected(_)
                | VolumeError::DeviceDisconnected(_)
                | VolumeError::DeviceSessionReset(_),
            )) => WriteOperationError::ConnectionInterrupted { path },
            _ => WriteOperationError::DestinationNotFound { path },
        }
    }

    /// The conflict scan's refusal of the box text `path`, in the volume's own vocabulary.
    pub(crate) fn into_scan_error(self, path: String) -> VolumeScanError {
        match self {
            Self::Unresolved(TypedPathRefusal::Unanswered(error)) => VolumeScanError::Volume { error },
            _ => VolumeScanError::Volume {
                error: VolumeError::NotFound(path),
            },
        }
    }
}

/// The destination box's text (person-typed, VOLUME-RELATIVE or absolute, a local `~`
/// already expanded by the caller) as the app path on `volume` a write lands on: text with
/// no `..` is [`anchored`], byte for byte as before. A `..` is the person's, so it's answered
/// the way `ls` would answer it: the part before it anchors on the volume's root, and the
/// typed-path resolver (`AppHost::resolve_typed`, `TypedBy::Person`) asks THIS volume, the
/// one doing the write, so `x/link/../dst` lands on the link target's sibling and the
/// probes, the scan, and the write all name that one place. Bounded by the resolver's 2 s
/// deadline.
///
/// `AppHost::on_volume`, ❌ not `AppHost::registry()`: the volume is already picked, and the
/// registry's longest-root pick ties between volumes sharing a root (every `InMemoryVolume`
/// sits at `/`) and would hand a mounted share's `..` to the share's volume when the write
/// goes through the boot volume's OS calls.
///
/// # Errors
///
/// [`DestinationRefusal`]: no anchor on this volume, a volume that couldn't answer a `..`,
/// or a `..` that led off the volume.
pub(crate) async fn destination(volume: &Arc<dyn Volume>, text: &str) -> Result<AppPathBuf, DestinationRefusal> {
    destination_over(volume.as_ref(), text, &AppHost::on_volume(Arc::clone(volume))).await
}

/// [`destination`] for a probe, with the volume that answers for the place it names: text with
/// no `..` keeps `volume` (the caller routed it already, `path_checks::box_volume`), and a
/// resolved `..` routes its place again, so `x/a.zip/sub/../y` is asked of the archive, as
/// `x/a.zip/y` typed plainly is. `volume_id` is `volume`'s registry id.
///
/// # Errors
///
/// [`DestinationRefusal`], as [`destination`].
pub(crate) async fn probed_destination(
    volume_id: &str,
    volume: Arc<dyn Volume>,
    text: &str,
) -> Result<(Arc<dyn Volume>, AppPathBuf), DestinationRefusal> {
    let target = destination(&volume, text).await?;
    if !has_dot_dot_name(text) {
        return Ok((volume, target));
    }
    let routed = get_volume_manager().resolve(volume_id, &target).await.volume;
    Ok((routed.unwrap_or(volume), target))
}

/// [`destination`] with each `..` answered by `host`.
async fn destination_over(volume: &dyn Volume, text: &str, host: &AppHost) -> Result<AppPathBuf, DestinationRefusal> {
    let root = volume.root();
    if !has_dot_dot_name(text) {
        return anchored(root, text).ok_or(DestinationRefusal::NoAnchor);
    }
    if !ESCAPES_LIVE && text.chars().any(is_escape) {
        return Err(DestinationRefusal::NoAnchor);
    }
    let slashed = if PathFlavor::HOST == PathFlavor::Windows && !is_scheme_url(text) {
        text.replace('\\', "/")
    } else {
        text.to_owned()
    };
    let (head, rest) = slashed.split_at(first_dot_dot(&slashed));
    // A bare leading `..` (the box anchors relative text on the root) starts at the root.
    let head = if head.split('/').all(|name| name.is_empty() || name == ".") {
        "/"
    } else {
        head
    };
    let anchor = anchored(root, head).ok_or(DestinationRefusal::NoAnchor)?;
    let typed_text = format!("{}/{rest}", anchor.as_str().trim_end_matches('/'));
    let typed = host
        .resolve_typed(&typed_text, None, TypedBy::Person)
        .await
        .map_err(DestinationRefusal::Unresolved)?;
    if !typed.path.is_on_volume(root) {
        return Err(DestinationRefusal::OffVolume);
    }
    Ok(typed.path)
}

/// The destination box text an engine start (or its conflict scan) takes on `volume_id`,
/// `..`-free: `None` when the text holds no `..` (the engine anchors it,
/// `cmdr_operations::resolve_dest_path`, as it always did) or names a volume nothing has
/// registered (the engine refuses that volume itself), else the place [`destination`]
/// resolves it to, spelled as the engine takes it.
///
/// # Errors
///
/// [`DestinationRefusal`].
pub(crate) async fn box_destination(volume_id: &str, text: &str) -> Result<Option<String>, DestinationRefusal> {
    if !has_dot_dot_name(text) {
        return Ok(None);
    }
    let Some(volume) = get_volume_manager().get(volume_id) else {
        return Ok(None);
    };
    // `~` expands for a local volume only, as `resolve_dest_path` expands it.
    let expanded = if volume.local_path().is_some() {
        Cow::Owned(cmdr_fs::tilde::expand_tilde(text))
    } else {
        Cow::Borrowed(text)
    };
    let path = destination(&volume, &expanded).await?;
    Ok(Some(path.into_string()))
}

/// [`box_destination`] for a write start: the text as sent, or the resolved place.
///
/// # Errors
///
/// [`DestinationRefusal::into_write_error`].
pub(crate) async fn transfer_destination(volume_id: &str, text: String) -> Result<String, WriteOperationError> {
    match box_destination(volume_id, &text).await {
        Ok(resolved) => Ok(resolved.unwrap_or(text)),
        Err(refusal) => Err(refusal.into_write_error(text)),
    }
}

/// The byte offset of the first `..` name in `/`-separated text, or its length.
fn first_dot_dot(text: &str) -> usize {
    let mut offset = 0;
    for name in text.split('/') {
        if name == ".." {
            return offset;
        }
        offset += name.len() + 1;
    }
    text.len()
}

/// `text` parsed with this host's flavor ([`crate::app_text::parse`]).
fn parsed(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    crate::app_text::parse(text)
}

/// [`parsed`] for the POSIX flavor, whatever the host: a volume-relative `/x` on Windows.
/// Text holding an escape-range code point has no literal POSIX spelling to read here, so
/// it's refused as one this door doesn't read.
fn parsed_posix(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    crate::app_text::parse_posix(text)
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

#[cfg(test)]
#[path = "wire_path_destination_test.rs"]
mod wire_path_destination_test;
