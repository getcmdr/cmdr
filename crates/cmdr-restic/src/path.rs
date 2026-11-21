//! Where the restic view lives in a path, what a path inside it names, and how
//! the crate joins a name under a folder.
//!
//! Both answers are lexical: no listing, no `stat`. A host gates every path
//! through [`restic_route`] on its hot path, and confirms the candidate repo
//! once, through the hub's memo.

use crate::snapshots::{LATEST_FOLDER_NAME, SHORT_ID_LEN};
use cmdr_fs::app_path::{
    AppName, AppPath, AppPathBuf, ESCAPES_LIVE, ListedFolder, NotAnAppPath, PathArithmetic, is_escape, unescape_bytes,
};
use std::borrow::Cow;

// The lexical naming lives in `cmdr-fs`, so the listing engine and the write
// guards can ask it without linking `rustic_core`; one definition, re-exported.
// A host confirms `restic_route`'s candidate through
// `ResticRepos::confirm_repo` before it routes.
pub use cmdr_fs::restic_view::{VIRTUAL_ENTRY_NAME, restic_route};

/// Which snapshot a folder name inside the view stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SnapshotRef {
    /// The `latest` folder.
    Latest,
    /// A dated folder, by the id prefix its name ends with.
    ShortId(String),
}

/// What a path inside the view names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VirtualPath {
    /// The view's own folder: the snapshot list.
    Root,
    /// A snapshot folder, or something inside one (`inner` empty for the folder
    /// itself), one name per level in app spelling. A snapshot's names are POSIX
    /// names from the repo, whatever OS made the backup.
    Snapshot(SnapshotRef, Vec<String>),
}

/// Classifies `path` against the view's root folder (`<repo>/Browse
/// snapshots`) by the app path's own `strip_prefix`, so a name holding `\` or
/// `:` stays one name on every OS. A relative `path` (only a transition-bridge
/// value is one; a typed path is absolute) is read against `view_root`.
///
/// `None` for a path outside the view, one that climbs (`..`), and a first
/// name that names no snapshot. A dated folder resolves by its id suffix
/// alone: the time in front is local time, so a time zone change or DST would
/// otherwise orphan every saved path.
pub(crate) fn classify(view_root: &AppPath, path: &AppPath) -> Option<VirtualPath> {
    let names: Vec<String> = match path.to_canonical() {
        Ok(_) => path
            .strip_prefix(view_root)?
            .names()
            .map(|name| spelled(name).into_owned())
            .collect(),
        Err(NotAnAppPath::Relative) => {
            let mut names = Vec::new();
            for name in path.names() {
                match name {
                    "." => {}
                    ".." => return None,
                    name => names.push(spelled(name).into_owned()),
                }
            }
            names
        }
        Err(_) => return None,
    };
    let mut names = names.into_iter();
    let Some(first) = names.next() else {
        return Some(VirtualPath::Root);
    };
    let snapshot = if first == LATEST_FOLDER_NAME {
        SnapshotRef::Latest
    } else {
        SnapshotRef::ShortId(short_id_of(&first)?.to_string())
    };
    Some(VirtualPath::Snapshot(snapshot, names.collect()))
}

/// The id prefix a dated folder name ends with: `… (a1b2c3d4)`. Lowercase hex,
/// [`SHORT_ID_LEN`] to 64 digits, as the names are minted.
pub(crate) fn short_id_of(folder_name: &str) -> Option<&str> {
    let open = folder_name.strip_suffix(')')?.rfind('(')?;
    let hex = &folder_name[open + 1..folder_name.len() - 1];
    let well_formed = (SHORT_ID_LEN..=64).contains(&hex.len())
        && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    well_formed.then_some(hex)
}

/// The fixed name `name` (a repo folder, an id, the view's own name) inside
/// `dir`, by `ListedFolder`'s join: the app path's own for a typed folder, and
/// `Path::join`'s for a bridge folder with no canonical twin (the app's route
/// hands a repo on a `scheme://` parent over as `scheme:/…`; DETAILS § "App
/// paths").
///
/// # Panics
///
/// When `name` isn't one plain name: every caller passes a constant or an id.
pub(crate) fn child(dir: &AppPath, name: &str) -> AppPathBuf {
    let name = AppName::from_app_spelling(name).expect("a repo folder or an id is one plain name");
    ListedFolder::new(dir)
        .join(name)
        .expect("a plain name holding no `\\` joins any folder")
}

/// The text naming `name` inside `dir`, as a row or an error spells it: the
/// app path's own join when `dir` is canonical and `name` one plain name, else
/// the text `Path::join` made on macOS and Linux (`/` alone separates), so a
/// bridge folder (a trailing `/`, a `scheme:/` route root) and a name a hostile
/// repo stores (`a/b`, `..`) keep the row they had.
pub(crate) fn child_text(dir: &AppPath, name: &str) -> String {
    let joined = if dir.is_canonical()
        && let Ok(plain) = AppName::from_app_spelling(name)
        && let Ok(joined) = dir.join(plain)
    {
        joined.into_string()
    } else {
        posix_join_text(dir.as_str(), name)
    };
    match spelled(&joined) {
        Cow::Borrowed(_) => joined,
        Cow::Owned(literal) => literal,
    }
}

/// The text a row, an error, or a name match spells for app text, until
/// escapes go live (`cmdr_fs::app_path::ESCAPES_LIVE`): no app text holds a live
/// escape yet, so an escape in it is the transition bridge's lossless spelling
/// of a literal escape-range code point the wire carried (`caf\u{10FEE9}`), and
/// this spells that code point back, as `Path` did. Borrowed when there's none.
/// Once escapes go live an escape is a real name's byte, and the text stays.
pub(crate) fn spelled(text: &str) -> Cow<'_, str> {
    if ESCAPES_LIVE || !text.chars().any(is_escape) {
        return Cow::Borrowed(text);
    }
    match unescape_bytes(text) {
        Ok(bytes) => Cow::Owned(String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => Cow::Borrowed(text),
    }
}

/// [`spelled`] for a whole path: what an error names.
pub(crate) fn text_of(path: &AppPath) -> String {
    spelled(path.as_str()).into_owned()
}

/// `Path::join` on a POSIX host, over text: an absolute `name` replaces `dir`,
/// and a `/` goes between them unless `dir` is empty or already ends in one.
fn posix_join_text(dir: &str, name: &str) -> String {
    if name.starts_with('/') {
        return name.to_owned();
    }
    let slash = !dir.is_empty() && !dir.ends_with('/');
    let mut out = String::with_capacity(dir.len() + usize::from(slash) + name.len());
    out.push_str(dir);
    if slash {
        out.push('/');
    }
    out.push_str(name);
    out
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
