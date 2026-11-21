//! How [`ArchiveVolume`](super::ArchiveVolume) names things: the inner path a
//! volume path asks the index for, and the path an entry gets under the archive.
//!
//! Both are app-path arithmetic over the archive's own path (`strip_prefix`,
//! `join_relative`), so a remote archive's `sftp://…/a.zip` keeps its `//` and a
//! Windows `C:/x/a.zip` never gains a `\`. ❗ Two shapes keep the `std::path`
//! answer they had, so the wire stays byte-identical: an archive path or a
//! volume path that isn't canonical (only the transition bridge makes one, from
//! `ArchiveVolume::new`'s `PathBuf` or a caller's text), and text holding an
//! escape code point (a name that isn't valid UTF-8, which the row spells lossily
//! until escapes go live). Those go with the bridge: every caller hands a typed,
//! canonical path once the app converts, and an escaped name then joins as
//! itself.

use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathArithmetic, RelativeAppPath, is_escape};

use super::{from_std_view, std_view};

/// Whether `text` takes the typed arithmetic: canonical, and holding no escape.
fn is_plain(path: &AppPath) -> bool {
    path.is_canonical() && !path.as_str().chars().any(is_escape)
}

/// The archive-inner path the index keys on for `path` (`/`-separated, no
/// surrounding slashes, root is `""`): `path` below `archive`, or `path`'s own
/// names when it isn't under it. The index re-trims slashes, so a rough result
/// is fine. A bridge caller may also hand the root itself (empty, `.`) or an
/// already-inner relative path, which `Path`'s rules read as before.
pub(super) fn inner_path(archive: &AppPath, path: &AppPath) -> String {
    if is_plain(archive) && is_plain(path) {
        let relative = path.strip_prefix(archive);
        let text = relative.as_deref().map_or(path.as_str(), RelativeAppPath::as_str);
        return trimmed(text);
    }
    let path = std_view(path);
    if path.as_os_str().is_empty() || path.as_ref() == Path::new(".") {
        return String::new();
    }
    let archive = std_view(archive);
    let relative = path.strip_prefix(archive.as_ref()).unwrap_or(&path);
    trimmed(&relative.to_string_lossy())
}

/// The index's spelling of a relative path: `/` separators, no surrounding `/`.
fn trimmed(text: &str) -> String {
    text.replace('\\', "/").trim_matches('/').to_string()
}

/// The path of the entry at the inner path `inner` (as the index spells it)
/// under `archive`.
pub(super) fn entry_path(archive: &AppPath, inner: &str) -> EntryPath {
    if is_plain(archive)
        && !inner.is_empty()
        && !inner.chars().any(is_escape)
        && let Ok(relative) = RelativeAppPath::new(inner)
        && let Ok(joined) = archive.join_relative(relative)
    {
        return EntryPath::Typed(joined);
    }
    EntryPath::Native(std_view(archive).join(inner))
}

/// The text a row carries for the archive itself (`root()`'s, as a row).
pub(super) fn root_row_text(archive: &AppPath) -> String {
    if is_plain(archive) {
        return archive.as_str().to_owned();
    }
    std_view(archive).to_string_lossy().into_owned()
}

/// An entry's path, typed, or as `Path::join` spelled it for a shape the type
/// doesn't take (see the module docs).
pub(super) enum EntryPath {
    Typed(AppPathBuf),
    Native(PathBuf),
}

impl EntryPath {
    /// The row's `FileEntry.path` text, and the path a `NotFound` names.
    pub(super) fn row_text(self) -> String {
        match self {
            Self::Typed(path) => path.into_string(),
            Self::Native(path) => path.to_string_lossy().into_owned(),
        }
    }

    /// The typed path, for what a transfer keys on (a sequential extract's
    /// `source_path`), matching what the listing reports.
    pub(super) fn into_app(self) -> AppPathBuf {
        match self {
            Self::Typed(path) => path,
            Self::Native(path) => from_std_view(&path),
        }
    }
}
