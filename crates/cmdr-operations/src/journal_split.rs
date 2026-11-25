//! How the journal splits one end of a row into the (folder text, name) it stores:
//! a `std::path` end ([`journal_split`]), and an end the volume engine joined typed
//! ([`journal_split_app`]). Split out of `journal.rs`, whose job is the rows.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

#[cfg(not(target_family = "wasm"))]
use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::app_path::{AppPath, PathArithmetic, PathFlavor, is_scheme_url};

use super::super::wire_paths::engine_path;

/// Split one end of a row into the (folder text, name) the journal stores: the APP
/// path's folder and last name, so a Windows drive row reads `C:/Users/x` (the store
/// interns by `/`, `cmdr-operation-log`'s `store/dirs.rs`), ❌ never the native
/// `C:\Users\x`, which interned as one name under `/` and read back as
/// `/C:\Users\x`, a path no volume answers (#1351). A path with no last name yields
/// empty strings.
///
/// The engine's ends are still `std::path` values, so each crosses here once, through
/// the non-escaping door (`from_native_unicode`). One it refuses keeps the text it
/// always had ([`bridge_split`]), counted on nothing because nothing changes for it:
/// a name that isn't Unicode text (10f escapes it), an escape-range code point, a
/// relative or `..` path, and a remote volume's `sftp://…` or, on Windows, POSIX
/// `/device/…` text, which isn't a native path. On macOS and Linux an absolute
/// Unicode path's app text IS its native text, so those rows keep every byte
/// (`journal_split_tests.rs`).
pub(super) fn journal_split(path: &Path) -> (String, String) {
    // A remote volume's `scheme://` text is no native path.
    if !path.to_str().is_some_and(is_scheme_url)
        && let Some(split) = native_split(path)
    {
        return split;
    }
    bridge_split(&separated(path))
}

/// [`journal_split`] for an end the volume engine holds as an app path, which it
/// joined typed, so every `\` in it is a name character. On Windows such an end
/// splits by the app path's own `/` (a POSIX server's `a\b` is ONE name), where
/// [`separated`] reads every `\` of a `std::path` end as a separator. Everywhere
/// else, and for an end holding no `\`, it is `journal_split` of the end's
/// `std::path` spelling, byte for byte the row it always made.
pub(super) fn journal_split_app(path: &AppPath) -> (String, String) {
    journal_split_app_as(path, PathFlavor::HOST)
}

/// [`journal_split_app`] as a `host` build splits it, whatever this build's host is (so
/// every OS tests both).
fn journal_split_app_as(path: &AppPath, host: PathFlavor) -> (String, String) {
    // Canonical only: a bridge value still spelled natively (`C:\x\f.txt`, a Windows
    // suite's root) holds its `\` as separators, which `journal_split` reads as such.
    if host == PathFlavor::Windows && path.as_str().contains('\\') && path.is_canonical() {
        let name = path.file_name().unwrap_or_default().to_owned();
        let dir = path
            .parent()
            .map(|parent| parent.as_str().to_owned())
            .unwrap_or_default();
        return (dir, name);
    }
    journal_split(&engine_path(path))
}

/// A volume engine's bridge path as `/`-separated text. On Windows `std::path` joined its
/// names with `\`, and a `std::path` end the engine hands here holds no name with one (the
/// ends it joins typed, where `\` is a name character, come through [`journal_split_app`]),
/// so every `\` separates: a cross-volume move's `/photos\sub\b.txt` stores under
/// `/photos/sub`, ❌ never a folder named `photos\sub`, which a POSIX server's undo would
/// read as one name. Elsewhere `\` is a name character, as every POSIX row always read it.
fn separated(path: &Path) -> Cow<'_, Path> {
    match path.to_str() {
        Some(text) if PathFlavor::HOST == PathFlavor::Windows && text.contains('\\') => {
            Cow::Owned(PathBuf::from(text.replace('\\', "/")))
        }
        _ => Cow::Borrowed(path),
    }
}

/// A native end's app split, or `None` for one the non-escaping door refuses.
#[cfg(not(target_family = "wasm"))]
fn native_split(path: &Path) -> Option<(String, String)> {
    AppPathBuf::from_native_unicode(path).ok().map(app_split)
}

/// The browser build has no native paths: its every volume is in memory or remote.
#[cfg(target_family = "wasm")]
fn native_split(_path: &Path) -> Option<(String, String)> {
    None
}

/// [`journal_split`] for an end already typed. The folder text reuses the path's own
/// buffer (its parent is a prefix of it), so a row costs the two allocations the
/// `std::path` split always did: this runs once per journaled item.
#[cfg(not(target_family = "wasm"))]
fn app_split(path: AppPathBuf) -> (String, String) {
    let name = path.file_name().unwrap_or_default().to_owned();
    let dir_len = path.parent().map(|parent| {
        debug_assert!(path.as_str().starts_with(parent.as_str()), "a parent is a prefix");
        parent.as_str().len()
    });
    let mut dir = path.into_string();
    dir.truncate(dir_len.unwrap_or(0));
    (dir, name)
}

#[cfg(test)]
#[path = "journal_split_tests.rs"]
mod journal_split_tests;

/// The split every row had before ends were typed: `std::path`'s, as lossy text.
fn bridge_split(path: &Path) -> (String, String) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    (dir, name)
}
