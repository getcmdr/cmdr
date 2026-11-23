//! One listed name on its way into a destination path ([`ListedName`], the app path's
//! own typed join), and the walk's crossings back to `std::path` ([`on_volume`],
//! [`std_of`], [`into_std`]) for the bodies that still take it. A leaf: it depends on
//! nothing in the transfer engine, so the stale-temp reaps (`cleanup.rs`, the remote zip
//! edit's) join through it without closing a module cycle through `landing.rs`.
//! `DETAILS.md` § "Listed names are untrusted".

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, ListedFolder};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{ChildName, NotAChildName, VolumeError};
use std::borrow::Cow;
use std::ffi::OsStr;
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
    pub(super) text: Option<&'a str>,
    pub(super) app: Cow<'a, AppName>,
}

impl<'a> ListedName<'a> {
    /// A name off a listing row (`FileEntry.name`).
    ///
    /// ❗ Also refused where `ChildName` refuses it on this host: on Windows a
    /// name holding `\` or `:` (a POSIX server's `a\b`, `12:00 notes`). The
    /// typed join keeps such a name whole, but the bodies past it still take
    /// `std::path` ([`into_std`]), whose `parent` and `file_name` would split
    /// it there, and a child joined under the wrong folder is a write nobody
    /// picked. Goes once those bodies take app paths (the plan's 10a); on macOS
    /// and Linux it refuses nothing the plain-name check doesn't.
    pub(crate) fn new(text: &'a str) -> Result<Self, NotAChildName> {
        ChildName::new(text)?;
        Ok(Self {
            text: Some(text),
            app: AppName::from_listed_text(text)?,
        })
    }

    /// The last name of a `std::path` the driver hands in (a top-level
    /// destination), refused as `ChildName` refuses it on this host: `native`
    /// as the driver spelled it, and `app` the same name in the bridge's
    /// spelling of the whole path ([`on_volume`]), so the two name one file.
    pub(super) fn native(native: &'a OsStr, app: &'a str) -> Result<Self, NotAChildName> {
        ChildName::new(native)?;
        Ok(Self {
            text: native.to_str(),
            app: Cow::Borrowed(AppName::from_app_spelling(app)?),
        })
    }

    /// `dir/<name>`, the typed join, for a caller that names the place the
    /// child would take (a refusal's question, a failure's path).
    pub(crate) fn under(&self, dir: &AppPath) -> Result<AppPathBuf, VolumeError> {
        join_under(&ListedFolder::new(dir), &self.app)
    }
}

/// `name` in `dir`. The join refuses only on Windows: a drive or UNC folder
/// takes no name holding `\` (a POSIX server's `a\b`), which is the destination
/// refusing that name, per item.
pub(super) fn join_under(dir: &ListedFolder<'_>, name: &AppName) -> Result<AppPathBuf, VolumeError> {
    dir.join(name)
        .map_err(|_| VolumeError::from(NotAChildName(name.shown().into_owned())))
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
pub(super) fn on_volume(path: &Path) -> Cow<'_, AppPath> {
    AppPath::legacy_unchecked(path)
}

/// An app path in the `std::path` spelling the engine's bodies still take (the
/// exact native bytes back, the bridge's way out): ONE named crossing for this
/// module and the walk, until those bodies take app paths.
pub(super) fn std_of(path: &AppPath) -> Cow<'_, Path> {
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
