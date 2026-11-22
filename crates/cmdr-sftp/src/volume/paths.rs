//! Turning the paths the app addresses this volume with into remote paths.
//!
//! Every session-touching method starts here, so a wrong answer sends a real
//! request to a real, wrong place. The translation itself is
//! `cmdr_fs::volume::remote_paths`, shared with WebDAV: the app root carries the
//! `sftp://<user>@<host>:<port>` prefix, the server's tree hangs under it, and a
//! bare server-absolute path is ❌ REFUSED rather than anchored. That module's
//! header has the reasoning and the two ways of guessing that would each send a
//! request somewhere wrong.
//!
//! Server paths are POSIX bytes on every host: split on `/` alone, so a `\` or a
//! `:` the server lists is part of a name, ❌ never a `std::path` separator or drive.

use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::volume::VolumeError;

use super::SftpVolume;

impl SftpVolume {
    /// The absolute server-side path for `path`, or `NotFound` when `path` isn't
    /// on this volume.
    pub(super) fn to_remote_path(&self, path: &AppPath) -> Result<String, VolumeError> {
        self.root
            .to_remote(path)
            .ok_or_else(|| VolumeError::NotFound(path.to_string()))
    }

    /// The app path's TEXT for a server path, as a row's `FileEntry.path` carries it, or
    /// `None` when the server path is off this volume's root (that entry is dropped).
    ///
    /// ❗ `RemoteRoot`'s own text (`<prefix><normalized server path>`), ❌ never the typed
    /// `to_app`'s canonical one: the wire keeps today's bytes, and a server name the app path
    /// can't hold as plain text yet (a NUL, an escape-range code point) still gets its row,
    /// so one odd name never hides a file or fails a folder that opened before.
    pub(super) fn app_text_of(&self, remote: &str) -> Option<String> {
        self.root
            .to_app_path(remote)
            .map(|app| app.to_string_lossy().into_owned())
    }

    /// The path the APP addresses `path` by: the prefixed spelling the panes
    /// hold, so the listing-cache patcher and a pane agree on what a file is
    /// called.
    ///
    /// A refusal becomes "no patch to make": a listing-cache patch is a
    /// courtesy, and ❌ must never fail a mutation that already landed. That
    /// includes a server path no app path holds (`NotAppText`, a NUL): the pane
    /// re-lists it.
    pub(super) fn display_path_for(&self, path: &AppPath) -> Option<AppPathBuf> {
        self.root
            .to_remote(path)
            .and_then(|remote| self.root.to_app(&remote).ok())
    }

    /// [`Self::display_path_for`] for the `std::path` value the patcher's
    /// `PatchSource::patch_display_path` still hands in, over `RemoteRoot`'s
    /// `std::path` forms (the same arithmetic, POSIX bytes split on `/`).
    pub(super) fn display_path_for_std(&self, path: &Path) -> Option<PathBuf> {
        self.root
            .to_remote_path(path)
            .and_then(|remote| self.root.to_app_path(&remote))
    }
}

/// The last name of a normalized server path (`/srv/data/a.txt` → `a.txt`), or
/// `None` at the server's root. Split on `/` alone: a `\` is part of the name.
pub(super) fn remote_file_name(remote: &str) -> Option<&str> {
    remote.rsplit('/').next().filter(|name| !name.is_empty())
}

/// `parent/name` on the server, with the root's own slash not doubled.
pub(super) fn join_remote(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod paths_test;
