//! The OS's own trash, one arm per OS (`docs/guides/platform-seams.md`): macOS's
//! `trashItemAtURL`, the `trash` crate's XDG trash on Linux, and the Shell's
//! `IFileOperation` Recycle Bin on Windows. The browser build has no trash: `delete::trash`
//! isn't compiled there at all.

use std::path::{Path, PathBuf};

use super::super::super::mutation_error::MutationError;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows/mod.rs")]
mod imp;

const _: fn(&Path) -> Result<Option<PathBuf>, MutationError> = move_to_trash_sync;
const _: fn(&Path) -> Option<PathBuf> = trash_dir_for_path;
const _: fn(&Path) = forget_restored;
const _: fn(&Path) -> String = trash_folder_facts;

/// What a trash folder [`trash_dir_for_path`] answered says about itself, as log text: home or
/// volume trash, owner uid against our euid, mode, symlink, and `st_flags`, all numbers, never a
/// name. Only macOS has such a folder to read; the other arms' `None` from `trash_dir_for_path`
/// never reaches theirs.
pub(super) use imp::trash_folder_facts;

/// Tidies what the OS keeps beside an item it trashed, once a restore has renamed
/// the item at `in_trash` back out. Only Windows keeps anything (the `$I` record
/// beside a `$R` item); the others do nothing.
pub(crate) use imp::forget_restored;
/// Moves ONE item (a whole tree for a folder) to the OS's trash, atomically: a
/// refusal leaves it exactly where it was. Returns the item's **in-trash location**
/// when the OS reports one: the journal records it as the trash row's dest, so a
/// later restore knows where to move it back from. `None` means "trashed, but no
/// restore location recorded". A missing item is [`MutationError::NotFound`]
/// (`symlink_metadata`, so a dangling link counts as there); a refusal is
/// [`MutationError::TrashRefused`] with a typed reason, ❌ never a permanent delete.
pub use imp::move_to_trash_sync;
/// The trash folder that holds items trashed from `path`'s volume, or `None` when
/// there's no single folder to show ("nowhere to go", never a failure).
pub use imp::trash_dir_for_path;
