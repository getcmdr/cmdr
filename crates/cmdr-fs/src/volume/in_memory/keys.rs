//! The store's keys: how a path the double is handed becomes the app path it
//! keys an entry by (`normalize`), the parent, name, and subtree arithmetic over
//! those keys, done by the app path's own rules so a name holding `\` is one name
//! on every host, and the text a listing shows for a key.

use std::borrow::Cow;
use std::collections::HashMap;

use super::{InMemoryEntry, InMemoryVolume};
use crate::app_path::{AppName, AppPath, AppPathBuf, ListedFolder, PathArithmetic, PathFlavor, RelativeAppPath};
use crate::os_error::OsErrorClass;
use crate::volume::VolumeError;

impl InMemoryVolume {
    /// Normalizes a path relative to the volume root: empty or `.` is `/`, a path
    /// with a root stays as given, and a relative one goes under `/`.
    ///
    /// A SCHEME-shaped path (`mtp://device/1/DCIM`) counts as rooted: that's the
    /// whole path vocabulary of an MTP volume, and rooting it under `/` would make
    /// every lookup miss while the entries it was built with keep their real keys.
    /// A drive (`C:/x`) or `\`-led path is rooted only on a Windows host, as
    /// `Path::is_absolute` read it; on macOS and Linux `C:/x` keys as `/C:/x`.
    pub(super) fn normalize<'p>(&self, path: &'p AppPath) -> Cow<'p, AppPath> {
        let text = path.as_str();
        if !text.starts_with('/') && text.split('/').all(|name| name.is_empty() || name == ".") {
            Cow::Borrowed(slash())
        } else if has_root(text) {
            Cow::Borrowed(path)
        } else {
            Cow::Owned(under_slash(path))
        }
    }

    /// Gets the parent path of a given path: `/` at a root.
    pub(super) fn parent_of(path: &AppPath) -> &AppPath {
        match path.parent() {
            Some(parent) => parent,
            None => slash(),
        }
    }

    /// Moves a folder's date to now when an entry lands in it or leaves it, as on
    /// every real store, so an engine test can tell a folder dated after its
    /// contents landed from one dated before them.
    pub(super) fn touch_parent_of(entries: &mut HashMap<AppPathBuf, InMemoryEntry>, child: &AppPath) {
        if let Some(parent) = child.parent().and_then(|parent| entries.get_mut(parent))
            && parent.metadata.is_directory
        {
            parent.metadata.modified_at = Some(Self::now_secs());
        }
    }

    /// [`OsErrorClass::NotEmpty`] when `path` is a folder that holds something:
    /// the refusal `Volume::delete` and a forced `Volume::rename` both owe, and
    /// real data-safety logic leans on. It carries the host's `ENOTEMPTY`, as a
    /// real disk's refusal would.
    pub(super) fn refuse_if_held(
        entries: &HashMap<AppPathBuf, InMemoryEntry>,
        path: &AppPath,
    ) -> Result<(), VolumeError> {
        let is_dir = entries.get(path).is_some_and(|e| e.metadata.is_directory);
        if is_dir && entries.keys().any(|k| k.parent() == Some(path)) {
            return Err(VolumeError::io_of_class("Directory not empty", OsErrorClass::NotEmpty));
        }
        Ok(())
    }

    /// Every key below the FOLDER `from`, beside where a rename to `to` puts it (empty
    /// for a file or a missing `from`). Refused, before anything moves, when a
    /// child's name is one `to` can't hold.
    pub(super) fn subtree_moves(
        entries: &HashMap<AppPathBuf, InMemoryEntry>,
        from: &AppPath,
        to: &AppPath,
    ) -> Result<Vec<(AppPathBuf, AppPathBuf)>, VolumeError> {
        if !entries.get(from).is_some_and(|entry| entry.metadata.is_directory) {
            return Ok(Vec::new());
        }
        let mut moves = Vec::new();
        for old_key in entries.keys() {
            let Some(suffix) = old_key.strip_prefix(from).filter(|suffix| !suffix.is_empty()) else {
                continue;
            };
            let new_key = join_tail(to, &suffix).ok_or_else(|| VolumeError::IoError {
                message: crate::error_detail::ErrorDetail::at_path(
                    "a moved child's name doesn't fit under ",
                    shown(to),
                ),
                raw_os_error: None,
            })?;
            moves.push((old_key.clone(), new_key));
        }
        Ok(moves)
    }
}

/// The store's root, `/`: a valid value on every host, Windows included.
pub(super) fn slash() -> &'static AppPath {
    AppPath::from_normalized_as("/", PathFlavor::Posix).expect("`/` is a canonical app path")
}

/// `text` as `Path::display` showed the key it spells: escaped bytes decoded, then
/// spelled lossily (on Windows an escaped unit shows as U+FFFD, as `display` did).
/// What a listing's `FileEntry.path` and `name`, and an error's path, read.
pub(super) fn lossy(text: &str) -> String {
    match crate::app_path::unescape_bytes(text) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => crate::app_path::shown_name(text).into_owned(),
    }
}

/// [`lossy`] of a whole path.
pub(super) fn shown(path: &AppPath) -> String {
    lossy(path.as_str())
}

/// `path`'s last name, as a listing shows it (empty at a root).
pub(super) fn name_of(path: &AppPath) -> String {
    path.file_name().map(lossy).unwrap_or_default()
}

/// Whether `text` names its own root, as `Path::is_absolute` read it on this host,
/// or is scheme-shaped.
pub(super) fn has_root(text: &str) -> bool {
    let bytes = text.as_bytes();
    let windows_rooted = PathFlavor::HOST == PathFlavor::Windows
        && (text.starts_with('\\') || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'));
    text.starts_with('/') || text.contains("://") || windows_rooted
}

/// A relative `path` under `/`, as `Path::join` put it there: the typed join for a
/// canonical tail, else the text after a `/` as a listed row reads it. A climbing
/// relative path (`a/../b`) has no such place and stays as given, where no key sits.
pub(super) fn under_slash(path: &AppPath) -> AppPathBuf {
    let text = path.as_str();
    if let Ok(rel) = RelativeAppPath::new(text)
        && let Ok(joined) = slash().join_relative(rel)
    {
        return joined;
    }
    match AppPath::from_listed_row(&format!("/{text}")) {
        Ok(joined) => joined.into_owned(),
        Err(_) => path.to_owned(),
    }
}

/// `rel`'s names under `base`: the typed multi-name join, else one name at a time as
/// a listed folder joins them (a bridge `base` with no canonical twin joins by text,
/// as `Path::join` did). `None` for a name `base` can't hold.
pub(super) fn join_tail(base: &AppPath, rel: &RelativeAppPath) -> Option<AppPathBuf> {
    if let Ok(joined) = base.join_relative(rel) {
        return Some(joined);
    }
    let mut at = base.to_owned();
    for name in rel.names() {
        let name = AppName::from_app_spelling(name).ok()?;
        at = ListedFolder::new(&at).join(name).ok()?;
    }
    Some(at)
}
