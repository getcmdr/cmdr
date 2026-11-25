//! One listed name on its way into a destination path ([`ListedName`], the app path's
//! own typed join), and the walk's crossings back to `std::path` ([`on_volume`],
//! [`std_of`], [`into_std`]) for the bodies that still take it. A leaf: it depends on
//! nothing in the transfer engine, so the stale-temp reaps (`cleanup.rs`, the remote zip
//! edit's) join through it without closing a module cycle through `landing.rs`.
//! `DETAILS.md` § "Listed names are untrusted".

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, ListedFolder, PathArithmetic, is_escape, unescape_bytes};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{NotAChildName, VolumeError};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// One listed name, checked as one plain name before anything joins it: the
/// text its listing spelled (what the destination's name index and the
/// look-alike rules compare), and the app spelling the typed join takes.
///
/// ❗ A SOURCE listing comes from whoever answers it, and a hostile server or
/// device can list `../x` or `/x`: this is the one way such a name reaches a
/// destination path, so it lands inside the folder or is refused
/// (`VolumeError::InvalidName`), ❌ never joined raw.
pub(crate) struct ListedName<'a> {
    /// The text as listed, `None` for a native name that isn't UTF-8.
    pub(super) text: Option<Cow<'a, str>>,
    pub(super) app: Cow<'a, AppName>,
}

impl<'a> ListedName<'a> {
    /// A name off a listing row (`FileEntry.name`), refused unless it's one
    /// plain name on every host (the empty name, `.`, `..`, a `/`, a NUL). A
    /// name holding `\` or `:` (a POSIX server's `a\b`, `12:00 notes`) is one
    /// name here on Windows too: the typed join keeps it whole and the leaf
    /// bodies take the app path, so a local NTFS destination refuses it by its
    /// rules, per item (`name_rules::DestinationNames`), and a remote one holds
    /// it.
    pub(crate) fn new(text: &'a str) -> Result<Self, NotAChildName> {
        Ok(Self {
            text: Some(Cow::Borrowed(text)),
            app: AppName::from_listed_text(text)?,
        })
    }

    /// The last name of an app path a driver hands in (a top-level
    /// destination), in the app's spelling: one plain name on every host, so a
    /// POSIX server's `a\b.txt` stays one (#1457). Its listed text is the native
    /// name it spells, `None` for one that isn't UTF-8, as the bridge's
    /// `std::path` name read it.
    pub(super) fn app(app: &'a str) -> Result<Self, NotAChildName> {
        Ok(Self {
            text: native_text(app),
            app: Cow::Borrowed(AppName::from_app_spelling(app)?),
        })
    }

    /// `dir/<name>`, the typed join, for a caller that names the place the
    /// child would take (a refusal's question, a failure's path).
    pub(crate) fn under(&self, dir: &AppPath) -> Result<AppPathBuf, VolumeError> {
        join_under(&ListedFolder::new(dir), &self.app)
    }
}

/// The native text an app-spelled name spells: itself when it holds no escape
/// (nearly every name), its decoded bytes when those are UTF-8 (a literal
/// escape-range code point the bridge escaped), else `None` (a Linux name that
/// isn't UTF-8, a Windows unpaired surrogate).
pub(crate) fn native_text(app: &str) -> Option<Cow<'_, str>> {
    if !app.chars().any(is_escape) {
        return Some(Cow::Borrowed(app));
    }
    let bytes = unescape_bytes(app).ok()?;
    String::from_utf8(bytes.into_owned()).ok().map(Cow::Owned)
}

/// The last name of `path` as a progress event shows it: the app path's own split
/// (a POSIX server's `a\b.txt` is one name), read as the lossy native text the
/// `std::path` name showed, so a Linux name that isn't UTF-8 reads as it did.
pub(crate) fn label_of(path: &AppPath) -> Option<String> {
    let name = own_name(path)?;
    Some(match unescape_bytes(&name) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => name.into_owned(),
    })
}

/// A top-level source's own last name, in the app's spelling: the app path's split
/// (a POSIX server's `a\b.txt` is one name), and at a ROOT, which has no last name to
/// the app path, the one `std::path` read there, so a root keeps the name it always
/// landed under: `sftp://u@h:22/` lands as `u@h:22`, `smb://nas/share/` as `share`, and
/// on macOS and Linux `C:/` as `C:` and `//server/share` as `share`. `/`, a Windows drive
/// or UNC root, and a bridge value ending in `..` have none.
pub(crate) fn own_name(path: &AppPath) -> Option<Cow<'_, str>> {
    if let Some(name) = path.file_name() {
        return Some(Cow::Borrowed(name));
    }
    if !path.is_root() {
        return None;
    }
    let native = std_of(path);
    // A root is app text with no escape in it (a scheme authority, a drive letter, a share), so its
    // `std::path` name is that text.
    let name = native.file_name()?.to_str()?;
    Some(Cow::Owned(name.to_owned()))
}

/// `name` in `dir`. The join refuses only on Windows: a drive or UNC folder
/// takes no name holding `\` (a POSIX server's `a\b`), which is the destination
/// refusing that name, per item.
pub(super) fn join_under(dir: &ListedFolder<'_>, name: &AppName) -> Result<AppPathBuf, VolumeError> {
    dir.join(name)
        .map_err(|_| VolumeError::from(NotAChildName(name.shown().into_owned())))
}

/// Where a TOP-LEVEL source lands in the destination folder a driver was handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TopLevel {
    /// At this path: the folder itself for a source with no name of its own (a
    /// volume's root), else its chosen name or its own, joined typed.
    At(AppPathBuf),
    /// The folder can't hold the name (a drive or UNC folder takes no `\`: a POSIX
    /// server's `a\b.txt` copied onto NTFS). The path is display text only, for
    /// the per-item question; nothing is written for it.
    Refused(AppPathBuf),
}

/// Where `source` lands in `dest_root`, as `chosen` (a name its caller picked:
/// `TransferLeaves`, a rename's target name) or its own last name: the app path's
/// split, so a POSIX server's `a\b.txt` is one name on every host, where
/// `std::path`'s Windows split landed it as `b.txt` (#1457).
///
/// A chosen name is app text (`TransferLeaves::chosen_name`, `TargetNames::chosen_for`),
/// read as one name by the app path's rules; one the folder can't hold answers
/// [`TopLevel::Refused`], ❌ never the source's own name: landing it under a name the
/// person didn't pick is worse than asking.
pub(crate) fn top_level(dest_root: &AppPath, source: &AppPath, chosen: Option<&str>) -> TopLevel {
    let (name, shown): (Option<Cow<'_, AppName>>, Cow<'_, str>) = match chosen {
        Some(text) => (AppName::from_listed_text(text).ok(), Cow::Borrowed(text)),
        None => match own_name(source) {
            Some(own) => (
                AppName::from_app_spelling(&own)
                    .ok()
                    .map(|name| Cow::Owned(name.to_owned())),
                own,
            ),
            None => return TopLevel::At(dest_root.to_owned()),
        },
    };
    match name.and_then(|name| ListedFolder::new(dest_root).join(&name).ok()) {
        Some(path) => TopLevel::At(path),
        None => TopLevel::Refused(shown_under(dest_root, &shown)),
    }
}

/// `name` under `dir` as display text only, for the question about a name `dir`
/// can't hold: the folder's text and the name as listed. ❌ Never written to.
pub(crate) fn shown_under(dir: &AppPath, name: &str) -> AppPathBuf {
    on_volume(Path::new(&format!("{}/{name}", dir.as_str().trim_end_matches('/')))).into_owned()
}

/// A listed child's own path (`FileEntry.path`, which the backend that listed it
/// built), typed: the one way a walk reads a child it descends into or acts on
/// (`AppPath::from_listed_row`), ❌ never `Path::new(&entry.path)` and the bridge
/// back, which re-escapes a live escape as literal text once escapes are live.
/// Borrowed for a plain row; byte for byte the bridge's value of canonical text.
///
/// A row holding a `..` name (a climbing path a hostile server or archive listed) is
/// refused as that child's name, ❌ never walked. An archive's relative inner row
/// (`dir/a.txt`) walks as the bridge passed it. ❗ An absolute row outside the listed
/// folder passes as it always did (#1444).
pub(crate) fn listed_path(entry: &FileEntry) -> Result<Cow<'_, AppPath>, VolumeError> {
    AppPath::from_listed_row(&entry.path).map_err(|_| VolumeError::from(NotAChildName(entry.name.clone())))
}

/// A `std::path` value the driver hands in, in the volume's spelling: the
/// bridge's lossless door in, ONE named crossing for this module and the walk.
pub(crate) fn on_volume(path: &Path) -> Cow<'_, AppPath> {
    AppPath::legacy_unchecked(path)
}

/// An app path in the `std::path` spelling the engine's bodies still take (the
/// exact native bytes back, the bridge's way out): ONE named crossing for this
/// module and the walk, until those bodies take app paths.
pub(crate) fn std_of(path: &AppPath) -> Cow<'_, Path> {
    path.as_std_legacy()
}

/// [`std_of`], owned, reusing the app path's buffer when the two spellings are
/// one (every name that's valid UTF-8 text holding no escape-range code
/// point), so a child's path costs the one allocation its join already made.
pub(crate) fn into_std(path: AppPathBuf) -> PathBuf {
    let respelled = match std_of(&path) {
        Cow::Borrowed(_) => None,
        Cow::Owned(native) => Some(native),
    };
    respelled.unwrap_or_else(|| PathBuf::from(path.into_string()))
}

#[cfg(test)]
#[path = "listed_name_tests.rs"]
mod tests;
