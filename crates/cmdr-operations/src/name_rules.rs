//! Which names a folder's filesystem takes: the ONE answer every new-name check
//! asks (the rename editor's validity check, the rename itself, New folder and
//! New file, Multi-Rename, and the frontend's keystroke mirror through the
//! host's command), keyed on the destination's `FilesystemKind`, ❌ never on
//! the host OS.
//!
//! A path the host's `std::fs` reaches is probed (`statfs` on macOS,
//! `/proc/mounts` on Linux), so an exFAT stick or a Finder-mounted share
//! answers for its own format. A volume serving its own I/O answers by its
//! backend: an SMB share is a share whatever the server's disk, an MTP phone and
//! every other backend refuse only what no filesystem takes, and leave the rest
//! to the device or server.

use std::path::{Path, PathBuf};

use cmdr_fs::filesystem_kind::FilesystemKind;
use cmdr_fs::validation::{NameRules, ValidationError, validate_filename};
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

/// The format a local probe finds at `path`: `Other` when it can't tell.
/// ❗ One `statfs` (a `/proc/mounts` read on Linux), which a hung network mount can hold.
fn probe_local(path: &Path) -> FilesystemKind {
    crate::local_disk::filesystem_at(path).map_or(FilesystemKind::Other, |filesystem| filesystem.kind)
}

/// The name rules of the filesystem holding `dir` on `volume_id`, for the
/// engine's own async callers ([`Operations::name_rules_at`](crate::Operations::name_rules_at)
/// is the host's door).
///
/// A local probe runs on the blocking pool, never on an async worker. An
/// unregistered volume, or a probe that can't answer, gets
/// [`NameRules::Posix`]: the write itself still answers for what the
/// destination refuses.
pub(crate) async fn name_rules_at(host: &EngineHost, volume_id: &str, dir: &Path) -> NameRules {
    let kind = match probe_for(host, volume_id, dir) {
        Probe::Known(kind) => kind,
        Probe::LocalPath(path) => host
            .runtime()
            .spawn_blocking(move || probe_local(&path))
            .await
            .unwrap_or(FilesystemKind::Other),
    };
    kind.name_rules()
}

/// Refuses a new name the folder's filesystem doesn't take: a `/` or a null
/// byte anywhere, a reserved device name on a share, a `:` on an exFAT stick.
/// Create and rename ask it before any I/O, so the refusal names the rule
/// rather than leaving the volume to answer with whatever its backend makes of
/// the name (Samba stores `CON` and lists it back as `CR23RR~9`).
pub(crate) async fn check_new_name(
    host: &EngineHost,
    volume_id: &str,
    dir: &Path,
    name: &str,
) -> Result<(), MutationError> {
    match validate_filename(name, name_rules_at(host, volume_id, dir).await) {
        Ok(()) => Ok(()),
        Err(ValidationError::Empty) => Err(MutationError::NameEmpty),
        Err(reason) => Err(MutationError::NameNotAllowed { reason }),
    }
}

impl crate::Operations {
    /// The name rules of the filesystem holding `dir` on `volume_id`: what the
    /// engine's own create and rename checks apply, for a host's keystroke hint
    /// (asked once per editor) and Multi-Rename's preview.
    /// ❗ Blocks for one local mount probe, so ❌ never call it on an async worker:
    /// Multi-Rename's open already runs on the blocking pool, and the IPC command
    /// hands it one.
    pub fn name_rules_at(&self, volume_id: &str, dir: &Path) -> NameRules {
        match probe_for(self.host(), volume_id, dir) {
            Probe::Known(kind) => kind.name_rules(),
            Probe::LocalPath(path) => probe_local(&path).name_rules(),
        }
    }
}

#[cfg(test)]
#[path = "name_rules_test.rs"]
mod tests;
