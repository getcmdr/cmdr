//! Shared detection and path primitives every archive-edit route builds on: the
//! archive-boundary inner-path helpers, the zip-only write guard (the single
//! write-side chokepoint that refuses tar/7z), and the duplicate-existence
//! oracle the create/rename pre-check consults. The per-flow routes (`copy_into`,
//! `move_out`, `driver`) layer their planning and drivers on top of these.

use cmdr_fs::app_path::AppPath;
use std::path::Path;
use std::sync::Arc;

use super::super::host::EngineHost;
use super::super::types::{ReadOnlySide, WriteOperationError};
use cmdr_archive::{ArchiveFormat, ArchiveIndex, LocalFileSource};
use cmdr_fs::volume::RoutedKind;

/// Joins an archive-inner parent path and a new child name into a single
/// `/`-separated inner path (root-relative, no surrounding slashes).
pub(crate) fn join_inner_path(inner_parent: &Path, name: &str) -> String {
    let parent = inner_parent.to_string_lossy().replace('\\', "/");
    let parent = parent.trim_matches('/');
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// Normalizes an archive-inner path (from `archive_boundary_candidate`) to the
/// `/`-separated, surrounding-slash-free form the changeset uses.
pub(crate) fn normalize_inner_path(inner: &Path) -> String {
    inner.to_string_lossy().replace('\\', "/").trim_matches('/').to_string()
}

/// The read-only refusal for a path that should have crossed an archive boundary
/// but didn't confirm (a mislabeled or vanished `.zip`).
///
/// `side` is the caller's to state: a move OUT of an archive is refused because
/// the SOURCE can't give up its files, while a copy in or a delete inside is the
/// DESTINATION refusing a write. Getting it wrong sends the user to fix the half
/// that was fine.
pub(super) fn read_only_error(path: &Path, side: ReadOnlySide) -> WriteOperationError {
    WriteOperationError::ReadOnlyDevice {
        path: path.display().to_string(),
        device_name: None,
        side,
    }
}

/// Refuses a mutation on a NON-ZIP archive. tar / 7z (and every other format) are
/// browse + extract only — only zip is writable — so every archive-edit route
/// funnels through this guard before touching the (zip-only) mutator. A tar/7z
/// target returns the typed read-only refusal, leaving the archive untouched. The
/// routing predicates stay format-agnostic (they also gate reads and navigation);
/// this is the single write-side chokepoint.
pub(crate) fn ensure_zip_writable(archive_path: &Path, side: ReadOnlySide) -> Result<(), WriteOperationError> {
    match cmdr_archive::format_for_path(archive_path) {
        Some(ArchiveFormat::Zip) => Ok(()),
        _ => Err(read_only_error(archive_path, side)),
    }
}

/// Whether `inner_path` already exists in the archive at `archive_path` (on its
/// parent drive `parent_volume_id`). The create/rename routing calls this to
/// reject a duplicate up front with the app's typed already-exists error, so the
/// mutator never builds a temp for an edit `zip` would only reject at write time
/// (`Duplicate filename`).
///
/// Dispatches on the parent like `run_managed_edit`: a LOCAL (or unregistered —
/// always a local edit) parent parses the central directory straight off the real
/// `.zip` file (zero network, no volume registration needed); a REMOTE (direct SMB
/// / MTP) parent has no local file to open, so it reads the central directory
/// through the parent volume (a ranged tail read via `resolve`, NOT a full pull).
///
/// An unresolvable or unreadable archive resolves to `false`: it isn't a known
/// duplicate, so routing proceeds and the managed op surfaces the real fault
/// through its normal error path rather than being masked here.
pub async fn archive_inner_exists(
    host: &EngineHost,
    parent_volume_id: &str,
    archive_path: &Path,
    inner_path: &str,
) -> bool {
    // Plain `get`: only whether the parent drive is remote; the inner read below resolves.
    let parent = host.volumes().get(parent_volume_id);
    let is_remote = parent.as_ref().is_some_and(|p| !p.supports_local_fs_access());

    if is_remote {
        // Joined the app path's way: on Windows `Path::join` put a `\` after the
        // remote archive (`/share/a.zip\dir/x.txt`), a name no route serves, so a
        // duplicate went unseen until the edit itself failed. An inner path no typed
        // join takes (a `..`) isn't a known duplicate.
        let archive = AppPath::legacy_unchecked(archive_path);
        let full_path = if inner_path.is_empty() {
            archive.into_owned()
        } else {
            let Some(joined) = cmdr_fs::app_path::RelativeAppPath::new(inner_path)
                .ok()
                .and_then(|inner| archive.join_relative(inner).ok())
            else {
                return false;
            };
            joined
        };
        let resolved = host.volumes().resolve(parent_volume_id, &full_path).await;
        return match resolved.volume {
            Some(volume) if resolved.routed == Some(RoutedKind::Archive) => volume.exists(&full_path).await,
            _ => false,
        };
    }

    let archive_path = archive_path.to_path_buf();
    let inner_path = inner_path.to_string();
    host.runtime()
        .spawn_blocking(move || {
            let Ok(source) = LocalFileSource::open(&archive_path) else {
                return false;
            };
            // Mutation is zip-only, so the oracle always parses as zip (no password:
            // a zip's central directory is plaintext even when its entries aren't).
            let Ok(index) = ArchiveIndex::parse(Arc::new(source), ArchiveFormat::Zip, None) else {
                return false;
            };
            index.exists(&inner_path)
        })
        .await
        .unwrap_or(false)
}
