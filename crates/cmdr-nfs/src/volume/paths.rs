//! Turning the paths the app addresses this volume with into export-relative
//! remote paths, and the one place a symlink's target is joined onto them.
//!
//! The translation itself is `cmdr_fs::volume::remote_paths::RemoteRoot`, shared
//! with SFTP and WebDAV: the app root carries `nfs://<host>:<port>/<export
//! segment>`, the export's tree hangs under it as `/`, and a bare
//! server-absolute path is ❌ REFUSED rather than anchored.

use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::volume::VolumeError;

use super::NfsVolume;

impl NfsVolume {
    /// The export-relative path for `path` (`/Photos/a.jpg`), or `NotFound`
    /// when `path` isn't on this volume.
    pub(super) fn to_remote_path(&self, path: &AppPath) -> Result<String, VolumeError> {
        self.root
            .to_remote(path)
            .ok_or_else(|| VolumeError::NotFound(path.as_str().to_owned()))
    }

    /// The path the APP addresses `remote` by, prefix and all: what a listing
    /// hands a pane and what the pane hands back. `None` off the export.
    ///
    /// ❗ Text, ❌ not a parsed app path: a row's `FileEntry.path` is the wire,
    /// and the export root's own row keeps its trailing `/`
    /// (`nfs://h:2049/Public/`), which a canonical app path drops.
    pub(super) fn app_path_of(&self, remote: &str) -> Option<String> {
        self.root
            .to_app_path(remote)
            .map(|path| path.to_string_lossy().into_owned())
    }

    /// The app spelling of `path`, for the listing-cache patcher. A refusal is
    /// "no patch to make": a patch is a courtesy, and ❌ must never fail a
    /// mutation that already landed.
    pub(super) fn display_path_for(&self, path: &Path) -> Option<PathBuf> {
        self.root
            .to_remote_path(path)
            .and_then(|remote| self.root.to_app_path(&remote))
    }

    /// [`display_path_for`](Self::display_path_for) for an app path, by the app
    /// path's own arithmetic. A server name no app path holds (a NUL) is no patch
    /// either.
    pub(super) fn app_path_for(&self, path: &AppPath) -> Option<AppPathBuf> {
        self.root
            .to_remote(path)
            .and_then(|remote| self.root.to_app(&remote).ok())
    }
}

/// `parent/name` in remote-path spelling.
pub(super) fn child_of(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

/// The components of a normalized remote path (`/a/b` → `["a", "b"]`; `/` →
/// none).
pub(super) fn components(remote: &str) -> impl Iterator<Item = &str> {
    remote.split('/').filter(|part| !part.is_empty())
}

/// `/` plus `parts` joined.
pub(super) fn path_of(parts: &[&str]) -> String {
    if parts.is_empty() {
        return "/".to_string();
    }
    let mut out = String::new();
    for part in parts {
        out.push('/');
        out.push_str(part);
    }
    out
}

/// Where a symlink in `parent` pointing at `target` leads, followed by `rest`,
/// as one export-relative path. `None` when the link can't be followed here:
/// its target is absolute (it names the SERVER's tree, which the export may
/// not even contain), it climbs out of the export, or it isn't UTF-8.
///
/// ❗ Lexical, the way the server's own `..` would read inside the export, and
/// checked as it goes: `../../etc` from `/a` leaves at its second `..`, ❌
/// never silently absorbed at the root the way a path's own `..` is.
pub(super) fn follow_link(parent: &[&str], target: &[u8], rest: &[&str]) -> Option<String> {
    let target = std::str::from_utf8(target).ok()?;
    if target.starts_with('/') || target.is_empty() {
        return None;
    }
    let mut out: Vec<&str> = parent.to_vec();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            name => out.push(name),
        }
    }
    out.extend_from_slice(rest);
    Some(path_of(&out))
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod paths_test;
