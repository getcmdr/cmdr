//! Storage paths: where an object sits on one storage, the session layer's own
//! spelling (`/DCIM/Camera`, `/` for the storage root).
//!
//! Held as typed app paths under the POSIX `/` root, so every split and join is
//! the app path's own, the same on every OS: a device name holding `\` or `:`
//! stays one name on Windows, ❌ never a `std::path` separator or drive. Callers
//! hand the session layer text (`DCIM/Camera`, `/DCIM/Camera`, ``); this module
//! is the one door that text comes through.
//!
//! Canonical, so the caches key one object by one value: `//`, `.`, and a
//! trailing `/` fold away (as `PathBuf`'s component-wise equality folded them),
//! and a `..` name or a stray NUL is no storage path at all.

use std::borrow::Cow;

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic, PathFlavor};

use super::errors::MtpConnectionError;

/// The storage root, `/`.
pub(crate) fn storage_root() -> &'static AppPath {
    AppPath::from_normalized_as("/", PathFlavor::Posix).expect("`/` is a canonical POSIX root")
}

/// `text` as the session layer spells it before canonicalizing: `/` for empty or `.`,
/// a leading `/` added to a relative spelling. What an error names, so a message
/// carries the path the caller asked about.
pub(crate) fn storage_text(text: &str) -> Cow<'_, str> {
    if text.is_empty() || text == "." {
        Cow::Borrowed("/")
    } else if !text.starts_with('/') {
        Cow::Owned(format!("/{text}"))
    } else {
        Cow::Borrowed(text)
    }
}

/// `text` as a storage path, or `None` when it holds a `..` name or a NUL that's no
/// pick (`cmdr_fs::twin_path`): no object can sit there.
pub(crate) fn storage_path(text: &str) -> Option<AppPathBuf> {
    AppPathBuf::parse(&storage_text(text), PathFlavor::Posix).ok()
}

/// [`storage_path`], or the `ObjectNotFound` an op on a path that can't exist answers.
pub(crate) fn storage_path_on(device_id: &str, text: &str) -> Result<AppPathBuf, MtpConnectionError> {
    storage_path(text).ok_or_else(|| MtpConnectionError::ObjectNotFound {
        device_id: device_id.to_string(),
        path: storage_text(text).into_owned(),
    })
}

/// `name`, as the device spells it, inside `folder`. `None` for a name that isn't one
/// plain name (empty, `.`, `..`, holding `/` or a NUL): no row can address it.
pub(crate) fn child(folder: &AppPath, name: &str) -> Option<AppPathBuf> {
    folder.join(AppName::from_app_spelling(name).ok()?).ok()
}

/// [`child`], and its text: a new name the device took that no path can address (a
/// rename to `a/b`) still reports where it went, joined by text, while the caches stay
/// out of it.
pub(crate) fn child_and_text(folder: &AppPath, name: &str) -> (Option<AppPathBuf>, String) {
    match child(folder, name) {
        Some(path) => {
            let text = path.as_str().to_string();
            (Some(path), text)
        }
        None if folder.is_root() => (None, format!("/{name}")),
        None => (None, format!("{}/{name}", folder.as_str())),
    }
}

/// The storage's URL, `mtp://{device}/{storage}`: the volume's root, and where every
/// change on it is reported. `None` for a device id the URL's authority can't hold (no
/// real device mints one: ids are `mtp-{bus}-{address}` or a serial).
pub(crate) fn storage_url(device_id: &str, storage_id: u32) -> Option<AppPathBuf> {
    let text = format!("mtp://{device_id}/{storage_id}");
    AppPath::from_normalized_as(&text, PathFlavor::Posix)
        .ok()
        .map(AppPath::to_owned)
}

/// `dir` (a storage path) at its storage's URL, `mtp://{device}/{storage}[/inner]`, in app
/// spelling: a storage path holds the device's literal text, and the URL is what the host
/// keys a pane by, so a name holding an escape-range code point goes through the
/// listed-text door, as the app spells that folder's row (#1310).
pub(crate) fn url_of(device_id: &str, storage_id: u32, dir: &AppPath) -> Option<AppPathBuf> {
    let url = storage_url(device_id, storage_id)?;
    if dir.is_root() {
        return Some(url);
    }
    AppPathBuf::from_listed_text(&format!("{}{}", url.as_str(), dir.as_str()), PathFlavor::Posix).ok()
}

/// The folder holding `path`, stepping over a twin's pick, and the storage root for the
/// root itself.
pub(crate) fn folder_or_root(path: &AppPath) -> &AppPath {
    super::twins::folder_of(path).unwrap_or(storage_root())
}

/// The extension `Path::extension` reads off a name (after the last `.`, none for a
/// leading-dot name like `.bashrc`), split on the name's own text: a device name holding
/// `\` is one name.
pub(crate) fn extension_of(name: &str) -> Option<&str> {
    if name == ".." {
        return None;
    }
    match name.rsplit_once('.') {
        Some(("", _)) | None => None,
        Some((_, extension)) => Some(extension),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_folds_to_one_canonical_storage_path() {
        for (text, canonical) in [
            ("", "/"),
            (".", "/"),
            ("/", "/"),
            ("DCIM", "/DCIM"),
            ("/DCIM/", "/DCIM"),
            ("//DCIM//Camera/./", "/DCIM/Camera"),
            ("/a\\b:c", "/a\\b:c"),
        ] {
            assert_eq!(
                storage_path(text).as_deref().map(AppPath::as_str),
                Some(canonical),
                "{text:?}"
            );
        }
        assert_eq!(storage_path("/DCIM/../x"), None, "a `..` names no object");
        assert_eq!(storage_path("/a\0b"), None, "a NUL that's no pick");
        assert_eq!(storage_text("DCIM/../x"), "/DCIM/../x", "an error names what was asked");
    }

    #[test]
    fn a_name_holding_a_backslash_or_colon_is_one_name_on_every_os() {
        let folder = storage_path("/Documents").expect("a storage path");
        let file = child(&folder, "a\\b 12:00.txt").expect("one plain name");
        assert_eq!(file.as_str(), "/Documents/a\\b 12:00.txt");
        assert_eq!(file.file_name(), Some("a\\b 12:00.txt"));
        assert_eq!(folder_or_root(&file), folder.as_app_path());
        for name in ["", ".", "..", "a/b", "a\0b"] {
            assert_eq!(child(&folder, name), None, "{name:?}");
        }
    }

    /// #1310: a folder the device names with an escape-range code point is reported at the
    /// URL the app holds for its row (the wire's spelling of the row text, which the bridge
    /// spells alike), ❌ never the literal text read as an escape (another name's bytes).
    #[test]
    fn a_report_url_spells_a_listed_escape_range_name_as_the_app_does() {
        use cmdr_fs::app_path::Namer;
        for name in ["x\u{10FEFF}", "\u{10E800}a"] {
            let dir = child(storage_root(), name).expect("a storage path");
            let url = url_of("mtp-0-1", 65537, &dir).expect("a URL");
            let row = format!("mtp://mtp-0-1/65537/{name}");
            assert_eq!(
                url,
                AppPathBuf::from_literal_store(&row, Namer::HOST).expect("an app path"),
                "{name:?}"
            );
            assert_eq!(
                cmdr_fs::app_path::listed_text(url.as_str()).as_deref(),
                Some(row.as_str())
            );
        }
        assert_eq!(
            url_of("mtp-0-1", 65537, storage_root())
                .map(AppPathBuf::into_string)
                .as_deref(),
            Some("mtp://mtp-0-1/65537")
        );
    }

    #[test]
    fn an_extension_reads_as_path_reads_one() {
        for (name, extension) in [
            ("a.JPG", Some("JPG")),
            (".bashrc", None),
            ("noext", None),
            ("a.b\\c", Some("b\\c")),
            ("..", None),
            ("a.", Some("")),
        ] {
            assert_eq!(extension_of(name), extension, "{name:?}");
        }
    }
}
