//! Translating between the paths the app addresses this volume with and the
//! share-relative paths smb2 speaks.
//!
//! Every session-touching method starts here, so a wrong answer sends a real
//! request to a real, wrong place. The rules that guard against that are on
//! [`SmbVolume::to_smb_path`].
//!
//! App paths and share paths are both `/`-separated text, read here by their own
//! bytes on every host: a `\` or a `:` is part of a name, ❌ never a `std::path`
//! separator or drive (on Windows `Path` would split `a\b` and read `C:` as a drive).

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic, listed_row_name, listed_text};
use cmdr_fs::volume::VolumeError;

use super::SmbVolume;

impl SmbVolume {
    /// Converts an app path on this volume to the SMB relative path string.
    ///
    /// The frontend sends paths under the volume root (which is the mount path).
    /// smb2 expects paths relative to the SHARE root with `/` separators, and the
    /// two are the same place only when this mount sits at the share root: an
    /// anchored mount (a DFS sub-mount, a subdirectory mount) has the volume root
    /// a directory or more inside the share, so its
    /// [`share_root`](SmbVolume::share_root) is joined on here.
    ///
    /// **Every name goes out byte-for-byte as given, never Unicode-normalized.** An
    /// SMB name is opaque bytes the server matches exactly, and one directory can
    /// hold its own name composed and its children's decomposed (ERR-VETBX): a path
    /// that came out of a listing is already the server's spelling in every
    /// component, and any fold here breaks the components it changes. ❌ Don't
    /// fold a foreign (typed, restored, macOS-mount) path here to "fix" it: no
    /// single form spells a mixed-form path, so the fix is a resolve against a real
    /// listing. `DETAILS.md` § "SMB names are opaque bytes".
    ///
    /// An absolute path outside the mount root is `NotFound`, and the root is
    /// matched by whole COMPONENTS: every way of guessing an answer here put a
    /// real request at a real, wrong place. Both, and what each caller does with
    /// the error: `backends/DETAILS.md` § "Per-backend decisions".
    ///
    /// ❗ Until escapes go live (`cmdr_fs::app_path::ESCAPES_LIVE`), an app path spells a
    /// listed escape-range code point as its own units' escapes, so it goes to the server as
    /// the text it stands for (`listed_text`, as `RemoteRoot::to_remote` sends it, #1310):
    /// a name listed as `x\u{10FEFF}` comes back as listed. Text no listed name escapes to
    /// (a live escape) is `NotFound`.
    pub(super) fn to_smb_path(&self, path: &AppPath) -> Result<String, VolumeError> {
        let listed = listed_text(path.as_str()).ok_or_else(|| VolumeError::NotFound(path.to_string()))?;
        self.smb_path_of(&listed)
    }

    /// [`Self::to_smb_path`] over LISTED text (a row's path, or an app path's text through
    /// `listed_text`), which a transition-bridge value spells as its caller did: relative,
    /// `.`, `//`, or with a trailing `/`. Read the way POSIX `Path` reads it, so every
    /// spelling a caller sends today reaches the server as it did.
    pub(super) fn smb_path_of(&self, text: &str) -> Result<String, VolumeError> {
        // Empty, `.`, and `/` all mean the volume root, which is where this mount
        // is anchored inside the share.
        if text.is_empty() || text == "/" || text == "." {
            return Ok(self.share_root.clone());
        }

        // Relative paths are relative to the VOLUME root, so they get the anchor
        // too, spelled as they came.
        if !text.starts_with('/') {
            return Ok(self.under_share_root(text));
        }

        // Absolute (the frontend does send these): must be inside the mount.
        match below_mount_root(text, self.mount_path.app().as_str()) {
            Some(relative) => Ok(self.under_share_root(relative)),
            None => Err(VolumeError::NotFound(text.to_string())),
        }
    }

    /// Joins a mount-relative path onto this mount's anchor inside the share.
    ///
    /// Free when the mount sits at the share root, which is every ordinary mount.
    fn under_share_root(&self, mount_relative: &str) -> String {
        match (self.share_root.as_str(), mount_relative) {
            ("", _) => mount_relative.to_string(),
            (anchor, "") => anchor.to_string(),
            (anchor, rest) => format!("{anchor}/{rest}"),
        }
    }

    /// The app path for `path`'s own location on this share, or `None` when `path`
    /// isn't on this share at all.
    ///
    /// For the post-mutation listing-cache patches: the mutation has already
    /// succeeded by the time they run, so a path that doesn't convert must skip
    /// the notification rather than turn a done write into a reported failure.
    /// That includes a mount-relative `..` a bridge value can spell: it names no
    /// listing a pane holds.
    pub(super) fn display_path_for(&self, path: &AppPath) -> Option<AppPathBuf> {
        let smb_path = self.to_smb_path(path).ok()?;
        self.app_path_of(&smb_path)
    }

    /// The typed app path for a share-relative SMB path: [`Self::to_display_path`]'s
    /// place, built by the app path's own join under the mount root, so a name
    /// holding `\` or `:` stays one name on every host. Each name goes through the
    /// listed-name door (`AppName::from_listed_text`), so a server name holding an
    /// escape-range code point is the value the app hands back for its row (#1310).
    /// Empty and `.` names drop, as `Path`'s components drop them; `None` for a `..`.
    pub(super) fn app_path_of(&self, smb_path: &str) -> Option<AppPathBuf> {
        let mut path = self.mount_path.app().to_canonical().ok()?.into_owned();
        for name in self.below_share_root(smb_path).split('/') {
            if name.is_empty() || name == "." {
                continue;
            }
            path = path.join(&AppName::from_listed_text(name).ok()?).ok()?;
        }
        Some(path)
    }

    /// Returns the full absolute path for a share-relative SMB path (under the
    /// mount point).
    ///
    /// The inverse of [`to_smb_path`](Self::to_smb_path): this mount's anchor
    /// inside the share comes back off, by whole COMPONENTS, so a sibling that
    /// merely shares a name prefix (`domain.old` beside `domain`) can't be
    /// mis-stripped into a path on a directory nobody mounted.
    ///
    /// Callers pass a path this instance's `to_smb_path` produced, so it's always
    /// under the anchor. One that isn't stays under the mount root unchanged,
    /// which is what an unanchored mount does with every path anyway.
    ///
    /// The text a listing row carries, so the mount root reads as the OS spells it
    /// (`to_string_lossy`), as every row did before app paths.
    pub(super) fn to_display_path(&self, smb_path: &str) -> String {
        let mount_relative = self.below_share_root(smb_path);
        if mount_relative.is_empty() {
            self.mount_path.to_string_lossy().to_string()
        } else {
            format!("{}/{}", self.mount_path.display(), mount_relative)
        }
    }

    /// A share-relative path re-expressed relative to this mount's anchor.
    fn below_share_root<'a>(&self, smb_path: &'a str) -> &'a str {
        below_share_root(&self.share_root, smb_path).unwrap_or(smb_path)
    }
}

/// The names below a rooted `text` once `mount_root`'s are stripped off, as
/// `Path::strip_prefix` answers on POSIX: matched by whole components (`//` and
/// `.` names don't count), and what's left spelled as `text` spells it, its own
/// leading and trailing `/` and `.` names trimmed. `None` when `text` isn't under
/// `mount_root`.
///
/// Byte-identical to `Path::strip_prefix(...).to_string_lossy()` on macOS and
/// Linux for every `/`-led text (the module's tests pin the shapes), and the same
/// on Windows, where `Path` would split on `\` too.
fn below_mount_root<'a>(text: &'a str, mount_root: &str) -> Option<&'a str> {
    let mut rest = text;
    for want in posix_names(mount_root) {
        let (name, after) = next_name(rest)?;
        if name != want {
            return None;
        }
        rest = after;
    }
    Some(trim_dot_names(rest))
}

/// `text`'s names as POSIX `Path::components` reads a rooted path: split on `/`,
/// the empty and `.` names gone.
fn posix_names(text: &str) -> impl Iterator<Item = &str> {
    text.split('/').filter(|name| !name.is_empty() && *name != ".")
}

/// The next name in `text` (empty and `.` names skipped) and what follows it.
fn next_name(text: &str) -> Option<(&str, &str)> {
    let mut rest = text;
    loop {
        let rest_trimmed = rest.trim_start_matches('/');
        if rest_trimmed.is_empty() {
            return None;
        }
        let (name, after) = rest_trimmed.split_once('/').unwrap_or((rest_trimmed, ""));
        if name != "." {
            return Some((name, after));
        }
        rest = after;
    }
}

/// `text` with its leading and trailing separators and `.` names trimmed, as
/// `Components::as_path` trims a path's remainder.
fn trim_dot_names(text: &str) -> &str {
    let mut start = text;
    loop {
        let trimmed = start.trim_start_matches('/');
        match trimmed.strip_prefix('.') {
            Some(after) if after.is_empty() || after.starts_with('/') => start = after,
            _ => {
                start = trimmed;
                break;
            }
        }
    }
    let mut end = start;
    loop {
        let trimmed = end.trim_end_matches('/');
        match trimmed.strip_suffix('.') {
            Some(before) if before.is_empty() || before.ends_with('/') => end = before,
            _ => return trimmed,
        }
    }
}

/// The last name of a share path, split on `/` alone (a `\` is part of a name),
/// as POSIX `Path::file_name` answers: trailing `/` and `.` names skipped, and
/// `None` for a `..` or no name at all.
pub(super) fn smb_file_name(smb_path: &str) -> Option<&str> {
    match smb_path.rsplit('/').find(|name| !name.is_empty() && *name != ".") {
        Some("..") | None => None,
        found => found,
    }
}

/// `path`'s parent and its last name as a listing row spells it, for a
/// listing-cache patch: split by the app path's own arithmetic, so a name holding
/// `\` stays one name on every host. The name is the native name's text, lossy
/// for an escape, the spelling this backend's rows carry.
pub(super) fn parent_and_row_name(path: &AppPath) -> Option<(&AppPath, String)> {
    Some((path.parent()?, row_name(path.file_name()?)))
}

/// An app path's name as a listing row spells it (`listed_row_name`, `patching`'s
/// rule, so every backend's patches name rows alike).
pub(super) fn row_name(name: &str) -> String {
    listed_row_name(name).into_owned()
}

/// `smb_path` re-expressed relative to `share_root`, or `None` when it isn't
/// inside that anchor at all.
///
/// Matches whole COMPONENTS: `domain.old/x` starts with `domain` as a string but
/// is a different directory, and stripping it would name a path on a directory
/// nobody mounted. An empty `share_root` is the share itself, so everything is
/// inside it and comes back unchanged.
///
/// Shared with the watcher, which turns share-relative event paths into the
/// mount-relative ones the listing cache is keyed on. ❌ Not two copies of this
/// rule: they would drift, and each direction of a path conversion that
/// disagrees with the other patches a cache key nothing is watching.
pub(super) fn below_share_root<'a>(share_root: &str, smb_path: &'a str) -> Option<&'a str> {
    if share_root.is_empty() {
        return Some(smb_path);
    }
    match smb_path.strip_prefix(share_root) {
        Some("") => Some(""),
        Some(rest) => rest.strip_prefix('/'),
        None => None,
    }
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod paths_test;
