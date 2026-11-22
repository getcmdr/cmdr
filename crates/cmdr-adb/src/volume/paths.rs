//! Turning the paths the app addresses this volume with into device paths, and
//! back.
//!
//! The volume is rooted at `adb://<serial>` (`cmdr_fs::volume::adb_app_root`),
//! and the device's own tree hangs under it: `adb://R58M1/sdcard/DCIM` is the
//! device's `/sdcard/DCIM`. The translation is `cmdr_fs::volume::remote_paths`,
//! shared with SFTP and WebDAV, so a bare device-absolute path is ❌ REFUSED
//! rather than anchored. That module's header has why: an un-hinted resolver
//! sends a scheme-free `/sdcard/x` to the Mac's boot disk.
//!
//! Device paths are POSIX text on every host: split on `/` alone, so a `\` or a
//! `:` a phone lists is part of a name, ❌ never a `std::path` separator or drive.
//!
//! ❗ **One refusal is this backend's own: a `..` that climbs above the root.**
//! `RemoteRoot` resolves `..` lexically and absorbs it at `/`, which is right for
//! a server rooted deeper than `/` (the climb leaves the root and is refused by
//! the containment check) but silent here, where the root IS `/`:
//! `adb://R58M1/sdcard/../../etc` would quietly become `/etc`. So the climb is
//! counted first and refused, ❌ never anchored. A typed app path holds no `..`,
//! so only a transition-bridge value (text a `std::path` caller still sends) can
//! carry one.

use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::volume::VolumeError;

use super::AdbVolume;

impl AdbVolume {
    /// The absolute device-side path for `path`, or `NotFound` when `path` isn't
    /// on this volume.
    ///
    /// Accepts the root (canonical `adb://<serial>/` or the bare `adb://<serial>`
    /// `root()` answers) and this device's own `adb://<serial>/…` paths; a bridge
    /// value's relative text and root aliases (empty, `.`, `/`) resolve lexically,
    /// as `RemoteRoot` reads them. Refuses a bare device-absolute path, another
    /// device's prefix, and a `..` that climbs above the root.
    pub(super) fn to_device_path(&self, path: &AppPath) -> Result<String, VolumeError> {
        let refused = || VolumeError::NotFound(path.to_string());
        if self.climbs_above_the_root(path.as_str()) {
            return Err(refused());
        }
        self.paths.to_remote(path).ok_or_else(refused)
    }

    /// Whether `text`'s `..` names climb above this volume's root.
    pub(super) fn climbs_above_the_root(&self, text: &str) -> bool {
        climbs_above_the_root(text, self.root.app().as_str())
    }

    /// The app path's TEXT for a device path, as a row's `FileEntry.path` carries
    /// it: the exact inverse of [`to_device_path`](Self::to_device_path).
    ///
    /// ❗ The device's `/` comes back as the bare root, `adb://<serial>`, with no
    /// trailing slash: that is what the device provider's row and the frontend's
    /// `constructAdbPath` spell, and a pane compares the root by string. Anything
    /// else is `RemoteRoot`'s own text (`<prefix><normalized device path>`), ❌ never
    /// the typed `to_app`'s canonical one, so the wire keeps today's bytes. `None`
    /// is `RemoteRoot`'s refusal of a path off the root; a listing drops that entry.
    pub(super) fn app_text_of(&self, device: &str) -> Option<String> {
        if device == "/" {
            return Some(self.root.app().as_str().to_owned());
        }
        self.paths
            .to_app_path(device)
            .map(|app| app.to_string_lossy().into_owned())
    }

    /// The path the APP addresses `path` by, for the listing-cache patcher. A
    /// refusal becomes "no patch to make": a listing-cache patch is a courtesy
    /// and ❌ must never fail a mutation that already landed. That includes a
    /// device path no app path holds (`NotAppText`, a NUL): the pane re-lists.
    ///
    /// The device's `/` answers `root()` itself (the bare `adb://<serial>`), so
    /// the listing host hears the root folder by the spelling a pane holds.
    pub(super) fn display_path_for(&self, path: &AppPath) -> Option<AppPathBuf> {
        let device = self.to_device_path(path).ok()?;
        if device == "/" {
            return Some(self.root.app().to_owned());
        }
        self.paths.to_app(&device).ok()
    }

    /// [`Self::display_path_for`] for the `std::path` value the patcher's
    /// `PatchSource::patch_display_path` still names, over `RemoteRoot`'s
    /// `std::path` forms (the same arithmetic, POSIX bytes split on `/`). Nothing
    /// here calls it: every patch goes through the typed `patch_*_at` forms.
    pub(super) fn display_path_for_std(&self, path: &Path) -> Option<PathBuf> {
        if self.climbs_above_the_root(&path.to_string_lossy()) {
            return None;
        }
        let device = self.paths.to_remote_path(path)?;
        if device == "/" {
            return Some(self.root.as_path().to_path_buf());
        }
        self.paths.to_app_path(&device)
    }
}

/// Whether `path`'s `..` names would climb above the root it hangs off.
///
/// Read the way a POSIX `Path` reads it on every host (whole names split on `/`,
/// `.` dropped, a leading `/` or `./` kept apart), so the root's own names are
/// stripped only from text that starts with them, as `Path::strip_prefix` did.
fn climbs_above_the_root(path: &str, root: &str) -> bool {
    let path = posix_components(path);
    let root = posix_components(root);
    let names = if path.anchored == root.anchored && path.names.starts_with(&root.names) {
        &path.names[root.names.len()..]
    } else {
        &path.names[..]
    };
    let mut depth = 0usize;
    for name in names {
        if *name == ".." {
            match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return true,
            }
        } else {
            depth += 1;
        }
    }
    false
}

/// What a POSIX `Path::components` makes of `text`: its leading anchor (`/`,
/// `./`, or none) and its names, `..` included.
struct PosixComponents<'a> {
    anchored: Anchor,
    names: Vec<&'a str>,
}

#[derive(PartialEq, Eq)]
enum Anchor {
    Root,
    CurDir,
    None,
}

fn posix_components(text: &str) -> PosixComponents<'_> {
    let anchored = if text.starts_with('/') {
        Anchor::Root
    } else if text == "." || text.starts_with("./") {
        Anchor::CurDir
    } else {
        Anchor::None
    };
    let names = text.split('/').filter(|name| !matches!(*name, "" | ".")).collect();
    PosixComponents { anchored, names }
}

/// The last name of a normalized device path (`/sdcard/a.txt` → `a.txt`), or
/// `None` at the device's root. Split on `/` alone: a `\` is part of the name.
pub(super) fn device_file_name(device: &str) -> Option<&str> {
    device.rsplit('/').next().filter(|name| !name.is_empty())
}

/// `parent/name` on the device, with the root's own slash not doubled.
pub(super) fn join_device_path(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod paths_test;
