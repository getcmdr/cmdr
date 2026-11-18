//! Which names a folder's filesystem takes: the ONE answer every new-name check
//! asks (the rename editor's validity check, the rename itself, New folder and
//! New file, Multi-Rename, Ask Cmdr's bulk renames, and the frontend's keystroke
//! mirror through the host's command), keyed on the destination's
//! `FilesystemKind`, ❌ never on the host OS alone.
//!
//! A path the host's `std::fs` reaches is probed (`statfs` on macOS,
//! `/proc/mounts` on Linux), so an exFAT stick or a Finder-mounted share
//! answers for its own format, and its paths are held to the host's path API
//! limit. A volume serving its own I/O answers by its backend: an SMB share is
//! a share whatever the server's disk (paths to SMB2's 32,767 units), an MTP
//! phone and every other backend refuse only what no filesystem takes, and
//! leave the rest, path length included, to the device or server.

use std::path::{Path, PathBuf};

use cmdr_fs::filesystem_kind::FilesystemKind;
use cmdr_fs::validation::{
    HOST_PATH_LIMIT, LengthLimit, NameRules, PortabilityWarning, ValidationError, validate_filename,
};
use cmdr_fs::volume::BackendKind;

use super::host::EngineHost;
use super::mutation_error::MutationError;

/// Where the answer for a folder comes from.
enum Probe {
    /// The format is the backend's, whatever the path.
    Known(FilesystemKind),
    /// The host's mount table names the format at this path.
    LocalPath(PathBuf),
}

/// Which question answers for `dir` on `volume_id`. No I/O.
fn probe_for(host: &EngineHost, volume_id: &str, dir: &Path) -> Probe {
    if host.volumes().local_disk(volume_id).is_some() {
        return Probe::LocalPath(dir.to_path_buf());
    }
    // Plain `get`, not `resolve`: a new name lands in the folder's own volume,
    // and an archive's rules would be the zip's, not the disk's.
    match host.volumes().get(volume_id) {
        None => Probe::Known(FilesystemKind::Other),
        Some(volume) => match volume.backend_kind() {
            BackendKind::Smb => Probe::Known(FilesystemKind::Smb),
            BackendKind::Mtp => Probe::Known(FilesystemKind::Mtp),
            // `local_path`, ❌ never `supports_local_fs_access`, which defaults to
            // `true` and so says nothing about an in-memory or demo volume.
            BackendKind::Local if volume.local_path().is_some() => Probe::LocalPath(dir.to_path_buf()),
            _ => Probe::Known(FilesystemKind::Other),
        },
    }
}

/// The rules a backend's own kind gives: an SMB2 path's ceiling for a share,
/// none for a backend that answers for itself.
fn backend_rules(kind: FilesystemKind) -> NameRules {
    let path_limit = (kind == FilesystemKind::Smb).then_some(LengthLimit::SMB_PATH);
    kind.name_rules().with_path_limit(path_limit)
}

/// The rules a local probe finds at `path`: `Other`'s when it can't tell, and
/// the host's path API limit either way, since the host's path API reaches it.
/// A folder that doesn't exist yet (a transfer's "will be created") answers for
/// its nearest existing ancestor, the disk it'll be created on.
/// ❗ One `statfs` (a `/proc/mounts` read on Linux) plus a `stat` per missing
/// level, which a hung network mount can hold.
fn probe_local(path: &Path) -> NameRules {
    let existing = path
        .ancestors()
        .find(|ancestor| std::fs::symlink_metadata(ancestor).is_ok())
        .unwrap_or(path);
    crate::local_disk::filesystem_at(existing)
        .map_or(FilesystemKind::Other, |filesystem| filesystem.kind)
        .name_rules()
        .with_path_limit(Some(HOST_PATH_LIMIT))
}

/// The name rules of the filesystem holding `dir` on `volume_id`, for the
/// engine's own async callers ([`Operations::name_rules_at`](crate::Operations::name_rules_at)
/// is the host's door).
///
/// A local probe runs on the blocking pool, never on an async worker. An
/// unregistered volume, or a probe that can't answer, gets
/// [`NameRules::POSIX`]'s characters: the write itself still answers for what
/// the destination refuses.
pub(crate) async fn name_rules_at(host: &EngineHost, volume_id: &str, dir: &Path) -> NameRules {
    match probe_for(host, volume_id, dir) {
        Probe::Known(kind) => backend_rules(kind),
        Probe::LocalPath(path) => host
            .runtime()
            .spawn_blocking(move || probe_local(&path))
            .await
            .unwrap_or_else(|_| NameRules::POSIX.with_path_limit(Some(HOST_PATH_LIMIT))),
    }
}

/// Refuses a new name the folder's filesystem doesn't take: a `/` or a null
/// byte anywhere, `.` or `..`, a name past its length limit, a `:` on a stick
/// the host's driver refuses it on. Create and rename ask it before any I/O, so
/// the refusal names the rule rather than leaving the volume to answer with
/// whatever its backend makes of the name. A name the folder TAKES but won't
/// keep portable (a `:` on a Mac's exFAT stick, `CON` on a share) passes, with
/// its warning: the editors showed it before the person committed.
pub(crate) async fn check_new_name(
    host: &EngineHost,
    volume_id: &str,
    dir: &Path,
    name: &str,
) -> Result<Option<PortabilityWarning>, MutationError> {
    match validate_filename(name, name_rules_at(host, volume_id, dir).await) {
        Ok(warning) => Ok(warning),
        Err(ValidationError::Empty) => Err(MutationError::NameEmpty),
        Err(reason) => Err(MutationError::NameNotAllowed { reason }),
    }
}

/// Whether `path`'s last segment, as spelled, is `.` or `..`. `Path::file_name`
/// can't say: it reads `/docs/..` as no name and `/docs/.` as `docs`, so a
/// rename to `dir.join("..")` would otherwise check the PARENT's name.
pub(crate) fn ends_in_dot_name(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    let last = bytes
        .rsplit(|&byte| std::path::is_separator(char::from(byte)))
        .next()
        .unwrap_or(bytes);
    last == b"." || last == b".."
}

impl crate::Operations {
    /// The name rules of the filesystem holding `dir` on `volume_id`: what the
    /// engine's own create and rename checks apply, for a host's keystroke hint
    /// (asked once per editor), Multi-Rename's preview, and a transfer dialog's
    /// warning about the names its scan found.
    /// ❗ Blocks for one local mount probe, so ❌ never call it on an async worker:
    /// Multi-Rename's open already runs on the blocking pool, and the IPC command
    /// hands it one.
    pub fn name_rules_at(&self, volume_id: &str, dir: &Path) -> NameRules {
        match probe_for(self.host(), volume_id, dir) {
            Probe::Known(kind) => backend_rules(kind),
            Probe::LocalPath(path) => probe_local(&path),
        }
    }
}

#[cfg(test)]
#[path = "name_rules_test.rs"]
mod tests;
