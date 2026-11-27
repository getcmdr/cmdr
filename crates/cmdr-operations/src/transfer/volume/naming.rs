//! Naming a destination on a volume: the ` (N)` pick, and how it gets claimed.
//!
//! The candidates and the sequence rule are the shared ones
//! (`cmdr_operations::unique_name`); what's volume-specific is HOW a pick is
//! held: an `O_CREAT|O_EXCL` placeholder where the destination is local-FS
//! backed, an `exists()` probe everywhere else, and the operation's
//! `ClaimedNames` ledger over both. Kept out of `conflict.rs`, which decides
//! conflict POLICY and only asks this for a name.

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, ListedFolder, PathArithmetic};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::super::look_alike::{LookAlike, look_alike_in};
use super::super::super::unique_name::AppNameCandidates;
use super::super::super::unique_name::ClaimedNames;
use super::listed_name::std_of;
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::Volume;

/// A name this operation has taken, and whether taking it PUT SOMETHING ON DISK.
///
/// The two are one answer because only the reserving call knows. A local-FS
/// destination gets an `O_CREAT|O_EXCL` zero-byte placeholder, and a caller whose
/// write then never happens has to take that placeholder back: on disk it is
/// indistinguishable from a real, empty file the copy produced. Every other
/// backend gets a probe, which leaves nothing behind and must NOT be deleted (the
/// name may be free for a completely unrelated reason).
pub(super) struct ClaimedName {
    pub(super) path: AppPathBuf,
    /// `true` ⇒ a zero-byte placeholder exists at `path` right now.
    pub(super) reserved_on_disk: bool,
}

impl ClaimedName {
    /// A name found free by probing, with nothing written to hold it.
    fn probed(path: AppPathBuf) -> Self {
        Self {
            path,
            reserved_on_disk: false,
        }
    }
}

/// Finds a unique filename on a volume by appending " (1)", " (2)", etc.
///
/// On a **local-FS-backed** destination volume (`local_path().is_some()`) the
/// chosen name is atomically RESERVED with an `O_CREAT|O_EXCL` placeholder, the
/// same TOCTOU guard `unique_name::find_unique_name` uses for the local-FS copy
/// path. Without it, a concurrent writer (a second Cmdr op, a cloud-sync agent,
/// a backup tool) could land a real file at `name (N)` between our non-atomic
/// `exists()` probe and the streaming writer's create+truncate, and the copy
/// would silently clobber it. The streaming write then lands ON the placeholder
/// (the write site opens the dest with create+truncate), exactly like the
/// local-FS path's `needs_safe_overwrite` flow. The returned path is the volume
/// path; the placeholder is created at the resolved local path.
///
/// On backends without exclusive-create semantics (MTP / SMB / InMemory,
/// `local_path()` is `None`) we can't reserve, so we fall back to the
/// `exists()` probe and re-check existence immediately before returning to keep
/// the residual window as narrow as the backend allows.
///
/// A **directory** takes that probe branch on every backend, local-FS dest
/// included. The placeholder is a FILE, and one sitting where the copy is about
/// to create a directory makes the merge walk's `create_directory`
/// report `AlreadyExists`, so the walk would try to merge into it and list it.
/// Letting the merge walker create the directory itself is also what records it
/// in `CreatedPaths`, which a pre-created one would miss and rollback would then
/// leave behind.
///
/// Both branches record the pick in the operation's `ClaimedNames` ledger and
/// walk past what's already there, which is what the probe alone can't do for a
/// directory (never reserved) or for the concurrent driver resolving several
/// top-level sources at once. Without it `photo.jpg` and `photo (1).jpg`
/// duplicated together both land on `photo (2).jpg`.
///
/// Naming itself is not this function's business: the candidates come from
/// `unique_name::NameCandidates`, the same sequence the local-FS namer walks, so a
/// volume dest numbers identically (`photo (1).jpg` continues to `photo (2).jpg`,
/// and `is_directory` also picks the candidate KIND, so `my.dir` numbers whole).
/// This function owns only the reservation.
///
/// The candidates are the app path's own split and join
/// (`unique_name::AppNameCandidates`), so a POSIX server's `a\b.txt` numbers as
/// `a\b (1).txt` beside it on every host (#1470). The op's ledger and a local
/// placeholder key by the `std::path` spelling (`std_of`): a reservation only
/// ever lands on a local disk, whose names never hold a POSIX server's `\`.
pub(super) async fn find_unique_volume_name(
    dest_volume: &Arc<dyn Volume>,
    path: &AppPath,
    is_directory: bool,
    claimed: &ClaimedNames,
) -> ClaimedName {
    let local_root = dest_volume.local_path().filter(|_| !is_directory);
    let mut candidates = if is_directory {
        AppNameCandidates::for_directory(path)
    } else {
        AppNameCandidates::for_file(path)
    };

    loop {
        // A folder that can't hold the name (a drive folder and a `\`) has no
        // pick: the clash's own path goes back, and the write that follows refuses
        // to land on the occupied name. The walk and the drivers refuse such a name
        // before it ever reaches a resolver, so this is the fail-closed answer.
        // Why it can't clobber: `probed` reserves nothing (`reserved_placeholder` false), so
        // the landing expects the name free (`ExpectedFree`): a `CreateNew` write, which
        // refuses the occupied name and leaves its file alone.
        let Some(candidate) = candidates.current() else {
            return ClaimedName::probed(path.to_owned());
        };
        // A pick is a NEW name, so it takes the destination's spelling
        // (`Volume::spell_new_name`), whatever the clashing name it counts up
        // from was spelled in.
        let new_path = spelled_for(dest_volume, candidate);
        let new_native = std_of(&new_path).into_owned();

        if !claimed.claim(&new_native) {
            // Spoken for by another source of this same operation.
            candidates.advance();
            continue;
        }

        if let Some(root) = &local_root {
            // Local-FS dest: reserve the name with an O_CREAT|O_EXCL placeholder
            // so no concurrent writer can sneak a file in before our write lands.
            let local_path = resolve_local_path(&root.as_std_legacy(), &new_native);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&local_path)
            {
                Ok(_) => {
                    // On the op's ledger until a write lands on it, so a write
                    // that never does can't leave it behind.
                    claimed.hold_placeholder(&new_native);
                    return ClaimedName {
                        path: new_path,
                        reserved_on_disk: true,
                    };
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    candidates.advance();
                }
                Err(_) => {
                    // Anything else (parent unwritable, ENOSPC, …) leaks back to
                    // the caller's write attempt, which has its own error path.
                    return ClaimedName::probed(new_path);
                }
            }
        } else {
            // Non-local backend: best-effort `exists()` probe. Re-check right
            // before returning to keep the residual window as narrow as we can.
            // A byte-exact backend also holds names in other spellings, which
            // the probe can't see and which are just as taken.
            if !dest_volume.exists(&new_path).await && !holds_a_look_alike(dest_volume, &new_path).await {
                return ClaimedName::probed(new_path);
            }
            candidates.advance();
        }

        // Safety limit to prevent an infinite loop.
        if candidates.attempts() > 1000 {
            // Extremely unlikely to happen.
            return ClaimedName::probed(candidates.current().unwrap_or_else(|| path.to_owned()));
        }
    }
}

/// `path` with its leaf spelled the way `dest_volume` wants a new name.
fn spelled_for(dest_volume: &Arc<dyn Volume>, path: AppPathBuf) -> AppPathBuf {
    // A name holding an escape (a Linux name that isn't UTF-8) has no text to respell,
    // as its `std::path` name had none.
    let (Some(parent), Some(name)) = (
        path.parent(),
        path.file_name()
            .filter(|name| !name.chars().any(cmdr_fs::app_path::is_escape)),
    ) else {
        return path;
    };
    match dest_volume.spell_new_name(name) {
        Cow::Borrowed(_) => path,
        Cow::Owned(spelled) => AppName::from_listed_text(&spelled)
            .ok()
            .and_then(|spelled| ListedFolder::new(parent).join(&spelled).ok())
            .unwrap_or(path),
    }
}

/// Whether `path`'s folder holds its name under another Unicode spelling.
///
/// A listing that fails leaves the probe's "free" standing: the pick is a new
/// name either way, so the worst case is an identical-looking sibling, never a
/// write over somebody's file.
async fn holds_a_look_alike(dest_volume: &Arc<dyn Volume>, path: &AppPath) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    matches!(
        look_alike_in(dest_volume.as_ref(), &std_of(parent), name).await,
        Ok(LookAlike::One(_) | LookAlike::Several)
    )
}

/// Resolves a destination-volume path against a local-FS volume root, so the
/// O_EXCL reservation lands at the same local path the volume's streaming
/// writer will later resolve `new_path` to. The rule itself is
/// `cmdr_fs::volume::root_anchored`, which `LocalPosixVolume::resolve` uses too:
/// that shared rule IS the guarantee the two paths agree.
fn resolve_local_path(root: &Path, path: &Path) -> PathBuf {
    cmdr_fs::volume::root_anchored(root, path)
}

/// Removes the `O_EXCL` placeholder a `Rename` resolution reserved for a write
/// that never landed, and stops holding it on the op's ledger.
///
/// ❗ **Only while it is still EMPTY.** The placeholder is zero bytes by
/// construction, so a size means somebody else wrote there between the
/// reservation and now, and deleting a stranger's file to tidy up our own
/// reservation is a worse bug than the litter. Best-effort otherwise: a
/// destination that won't answer keeps the empty file, which is the same
/// outcome as before and no worse.
pub(super) async fn take_back_reservation(dest_volume: &Arc<dyn Volume>, path: &Path, claimed: &ClaimedNames) {
    // Released FIRST: once we've looked, a later sweep has no business looking
    // again, and the name may be someone else's by then.
    claimed.release_placeholder(path);
    let placeholder = AppPath::legacy_unchecked(path);
    match dest_volume.get_metadata(&placeholder).await {
        Ok(entry) if !entry.is_directory && entry.size.unwrap_or(0) == 0 => {
            if let Err(e) = dest_volume.delete(&placeholder).await {
                log::debug!(target: "copy", "couldn't take back the reservation at {}: {e}", LogPrivate::file_path(path), e = LogError(&e));
            }
        }
        Ok(_) => log::warn!(
            target: "copy",
            "the reservation at {} isn't an empty file any more, so it stays",
            LogPrivate::path(path)
        ),
        Err(_) => {}
    }
}

/// Takes back every placeholder this operation reserved that no write landed
/// on: a leaf the driver abandoned at its cancel-drain deadline (its future,
/// and the cleanup in it, just stops), a solid-archive extract that reserved
/// every clashing name up front and then failed or was cancelled partway, or
/// a leaf a failing sibling kept from ever starting.
///
/// Every copy and cross-volume move post-loop runs it, whatever the ending.
/// Safe on success too: every landed write released its placeholder
/// (`stream_pipe.rs::stream_pipe_file`, `sequential_extract.rs`), so what's left
/// is only ever ours and empty, and [`take_back_reservation`]'s size check
/// still spares anything someone wrote into.
pub(super) async fn take_back_unfilled_reservations(dest_volume: &Arc<dyn Volume>, claimed: &ClaimedNames) {
    let unfilled = claimed.take_unfilled_placeholders();
    if unfilled.is_empty() {
        return;
    }
    log::info!(
        target: "copy",
        "taking back {} ` (N)` reservation(s) no write landed on",
        unfilled.len()
    );
    for path in unfilled {
        take_back_reservation(dest_volume, &path, claimed).await;
    }
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;
