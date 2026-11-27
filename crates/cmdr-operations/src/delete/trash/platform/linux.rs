//! Linux: the `trash` crate's XDG trash.

use std::fs;
use std::path::{Path, PathBuf};

use super::super::super::super::mutation_error::MutationError;
use super::super::super::super::types::TrashRefusalKind;

/// Moves `path` to the trash via the `trash` crate. Returns `Ok(None)`: the crate
/// doesn't surface the in-trash location, so no restore location is recorded.
pub fn move_to_trash_sync(path: &Path) -> Result<Option<PathBuf>, MutationError> {
    if fs::symlink_metadata(path).is_err() {
        return Err(MutationError::NotFound {
            path: path.display().to_string(),
        });
    }

    // Linux has no NSError to classify, and the `trash` crate's own error kinds don't
    // map onto the macOS permission question the UI asks. `Other` keeps the offer off.
    trash::delete(path).map_err(|e| MutationError::TrashRefused {
        reason: TrashRefusalKind::Other,
        detail: e.to_string(),
        codes: None,
    })?;
    // The `trash` crate doesn't surface the in-trash location, so no restore
    // location is recorded (trash rollback is then unavailable on Linux).
    Ok(None)
}

/// Never reached: [`trash_dir_for_path`] names no folder here.
pub(in super::super) fn trash_folder_facts(_trash: &Path) -> String {
    "no trash folder to read".to_string()
}

/// Always `None`: the `trash` crate doesn't surface a trash location, and the XDG
/// layout has no single answer for a non-home volume either.
pub fn trash_dir_for_path(_path: &Path) -> Option<PathBuf> {
    None
}

/// Nothing to tidy: no restore location is recorded here.
pub(crate) fn forget_restored(_in_trash: &Path) {}
