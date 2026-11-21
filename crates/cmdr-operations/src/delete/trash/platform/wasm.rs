//! The browser build: no OS trash to move anything into.

use std::path::{Path, PathBuf};

use super::super::super::super::mutation_error::MutationError;

/// Refuses with `TrashNotSupported`: a browser has no trash.
pub fn move_to_trash_sync(_path: &Path) -> Result<Option<PathBuf>, MutationError> {
    Err(MutationError::TrashNotSupported)
}

pub fn trash_dir_for_path(_path: &Path) -> Option<PathBuf> {
    None
}

pub(crate) fn forget_restored(_in_trash: &Path) {}
