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

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cmdr_fs::filesystem_kind::FilesystemKind;
use cmdr_fs::validation::{
    HOST_PATH_LIMIT, LengthLimit, NameRules, PortabilityWarning, ValidationError, validate_filename,
};
use cmdr_fs::volume::{BackendKind, NameFlavor, Volume, only_native_join_refuses};

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

/// Which question answers for `dir` on `volume`, for a caller holding the volume
/// itself (a transfer's destination) rather than its id. No I/O.
fn probe_for_volume(volume: &dyn Volume, dir: &Path) -> Probe {
    match volume.backend_kind() {
        BackendKind::Smb => Probe::Known(FilesystemKind::Smb),
        BackendKind::Mtp => Probe::Known(FilesystemKind::Mtp),
        BackendKind::Local if volume.local_path().is_some() => Probe::LocalPath(dir.to_path_buf()),
        _ => Probe::Known(FilesystemKind::Other),
    }
}

/// The rules a backend's own kind gives: an SMB2 path's ceiling for a share,
/// none for a backend that answers for itself.
fn backend_rules(kind: FilesystemKind) -> NameRules {
    let path_limit = (kind == FilesystemKind::Smb).then_some(LengthLimit::SMB_PATH);
    kind.name_rules().with_path_limit(path_limit)
}

/// The rules a local probe finds at `path`: `Other`'s when it can't tell, and
/// the host's path API limit and its own refusals either way, since the host's
/// path API reaches it (`NameRules::for_host_local_disk`: on Windows, never
/// weaker than Win32's refusals, whatever the kind lookup says).
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
        .for_host_local_disk()
        .with_path_limit(Some(HOST_PATH_LIMIT))
}

/// The name rules of the filesystem holding `dir` on `volume_id`, for the
/// engine's own async callers ([`Operations::name_rules_at`](crate::Operations::name_rules_at)
/// is the host's door).
///
/// A local probe runs on the blocking pool, never on an async worker. An
/// unregistered volume, or a probe that can't answer, gets
/// [`NameRules::POSIX`]'s characters (a local one raised to the host's own
/// refusals): the write itself still answers for what the destination refuses.
pub(crate) async fn name_rules_at(host: &EngineHost, volume_id: &str, dir: &Path) -> NameRules {
    match probe_for(host, volume_id, dir) {
        Probe::Known(kind) => backend_rules(kind),
        Probe::LocalPath(path) => host
            .runtime()
            .spawn_blocking(move || probe_local(&path))
            .await
            .unwrap_or_else(|_| {
                NameRules::POSIX
                    .for_host_local_disk()
                    .with_path_limit(Some(HOST_PATH_LIMIT))
            }),
    }
}

/// The name rules of a local `dir`, as a local probe finds them: the local
/// engine's door. ❗ Blocks for one mount probe.
pub(crate) fn local_rules_at(dir: &Path) -> NameRules {
    probe_local(dir)
}

/// The name rules of `dir` on `volume`, [`name_rules_at`] for a caller holding
/// the volume. ❗ Blocks for one local mount probe: an async caller runs it on
/// the blocking pool.
pub(crate) fn name_rules_on(volume: &dyn Volume, dir: &Path) -> NameRules {
    match probe_for_volume(volume, dir) {
        Probe::Known(kind) => backend_rules(kind),
        Probe::LocalPath(path) => probe_local(&path),
    }
}

/// The names a transfer's destination REFUSES, resolved once per operation
/// (the destination folder and everything under it sit on one filesystem).
///
/// A transfer keeps its sources' names, so only a Windows class the
/// destination's tier refuses is held against them here: a character, a device
/// name, a trailing space or period. Those are the names a driver strips or
/// rewrites without an error (Linux's vfat turns `dup.` into `dup`, onto a
/// sibling, `docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`), so the
/// engine must refuse them before any write, ❌ never leave them to the landing.
/// Length and the rest stay the destination's to answer, as before.
/// Unresolved (a test, an operation that never set it) refuses nothing by its rules.
///
/// ❗ Resolved or not, it also refuses a name only this host's native join can't keep a
/// direct child (`cmdr_fs::volume::only_native_join_refuses`: on Windows a remote name
/// holding `\` or `:`, `12:00 notes`), since the engines still join names with `std::path`.
/// So such an item is asked about like any refused name, ❌ never ending the transfer.
/// Always `false` on macOS and Linux.
// DEFAULT-OK: the zero value is an unresolved cell, which claims no rules about the disk; it
// refuses nothing more than before this check existed, and the landing still answers for itself.
#[derive(Default)]
pub(crate) struct DestinationNames {
    rules: OnceLock<NameRules>,
    /// Whose native join the engine's `std::path` joins follow: this host's, unless a test
    /// presets Windows' to drive that refusal on any OS.
    join: OnceLock<NameFlavor>,
}

impl DestinationNames {
    /// Resolves the rules once; a second call (or a test's preset) keeps the first.
    pub(crate) fn resolve(&self, rules: impl FnOnce() -> NameRules) {
        let _ = self.rules.get_or_init(rules);
    }

    /// Presets the native join's flavor, so a test drives Windows' join refusal on any OS.
    #[cfg(test)]
    pub(crate) fn join_as(&self, flavor: NameFlavor) {
        let _ = self.join.set(flavor);
    }

    /// Whether this host's native join refuses `name`, which POSIX keeps whole.
    pub(crate) fn native_join_refuses(&self, name: &(impl AsRef<OsStr> + ?Sized)) -> bool {
        only_native_join_refuses(name, self.join.get().copied().unwrap_or(NameFlavor::HOST))
    }

    /// [`Self::resolve`] for a destination volume, its local probe on the
    /// blocking pool. A probe that can't finish refuses nothing more than POSIX
    /// (the floor a local disk gets on its host still applies).
    pub(crate) async fn resolve_on(&self, host: &EngineHost, volume: &std::sync::Arc<dyn Volume>, dir: &Path) {
        if self.rules.get().is_some() {
            return;
        }
        let volume = std::sync::Arc::clone(volume);
        let dir = dir.to_path_buf();
        let rules = host
            .runtime()
            .spawn_blocking(move || name_rules_on(volume.as_ref(), &dir))
            .await
            .unwrap_or_else(|_| NameRules::POSIX.for_host_local_disk());
        self.resolve(|| rules);
    }

    /// Whether the destination refuses `name` (one path segment): by its rules, or because
    /// this host's native join can't keep the name whole ([`Self::native_join_refuses`]).
    pub(crate) fn refuses(&self, name: &OsStr) -> bool {
        if self.native_join_refuses(name) {
            return true;
        }
        let Some(rules) = self.rules.get() else {
            return false;
        };
        // A name that isn't UTF-8 has no Windows class to check.
        let Some(name) = name.to_str() else {
            return false;
        };
        matches!(
            validate_filename(name, *rules),
            Err(ValidationError::DisallowedCharacter { .. }
                | ValidationError::ReservedName { .. }
                | ValidationError::EndsWithSpaceOrPeriod)
        )
    }

    /// The first segment of `relative` the destination refuses, if any.
    pub(crate) fn first_refused<'p>(&self, relative: &'p Path) -> Option<&'p OsStr> {
        relative.iter().find(|segment| self.refuses(segment))
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
