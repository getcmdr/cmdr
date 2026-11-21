//! Windows: the Recycle Bin, through the Shell's `IFileOperation` (Microsoft's
//! replacement for `SHFileOperationW` since Vista), one item per call.
//!
//! ❗ The Shell DELETES an item it can't recycle (a share, a removable or `subst`
//! drive, a bin that's full or turned off, an item too big for it), silently under
//! `FOF_NO_UI`. Three walls, each refusing as `NoTrashForVolume` (the item stays, and
//! the UI offers the permanent delete it asks consent for):
//!
//! - `bin.rs`, before the Shell: a drive that isn't a fixed disk, a bin turned off, and a
//!   FILE past the bin's size limit, which the Shell announces as a recycle and then
//!   deletes for good, so the sink can't see it.
//! - `shell.rs`'s sink: every delete announced without `TSF_DELETE_RECYCLE_IF_POSSIBLE`,
//!   and any delete past the first (the Shell fell back to a folder's children, which
//!   it does when the folder can't move whole).
//!
//! ❌ Never pass `FOF_WANTNUKEWARNING` instead: it shows the Shell's own dialog.
//!
//! The in-trash location is the Shell's `$R…` item under
//! `<drive>\$Recycle.Bin\<SID>\`, returned in the app spelling, so the trash undo
//! renames it back; [`forget_restored`] then deletes its `$I…` record, without
//! which the Recycle Bin would list an entry whose item is gone.

mod bin;
mod shell;

use std::fs;
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::volume::path_grammar::host_app_path;

use super::super::super::super::mutation_error::MutationError;
use super::super::super::super::types::TrashRefusalKind;

use bin::Bin;
use shell::Recycled;

/// Moves `path` to the Recycle Bin. On a worker thread: the call blocks for as long
/// as the Shell takes (a rename on the same drive, so a folder costs one move).
pub fn move_to_trash_sync(path: &Path) -> Result<Option<PathBuf>, MutationError> {
    if fs::symlink_metadata(path).is_err() {
        return Err(MutationError::NotFound {
            path: path.display().to_string(),
        });
    }
    // The Shell parses a plain path, ❌ never the verbatim `\\?\` form the file APIs take.
    let Some(plain) = shell_spelling(path) else {
        return Err(MutationError::TrashRefused {
            reason: TrashRefusalKind::Other,
            detail: "the Shell can't name this path (too long, or a name Windows can't store plainly)".into(),
        });
    };
    // Asked before the Shell: a file too big for the bin is one the sink never sees
    // (`bin.rs`), and a drive with no bin at all gets a second wall in front of the veto.
    let file_size = fs::symlink_metadata(path)
        .ok()
        .filter(fs::Metadata::is_file)
        .map(|m| m.len());
    if bin::verdict(&plain, file_size) == Bin::WouldDeleteForGood {
        return Err(MutationError::TrashRefused {
            reason: TrashRefusalKind::NoTrashForVolume,
            detail: "no Recycle Bin here can take it (not a fixed disk, the bin turned off, or the item past its size limit)".into(),
        });
    }
    let landed = match shell::recycle(&plain) {
        Ok(Recycled::InBin(landed)) => Some(host_app_path(landed)),
        // A success with no bin item is how the Shell reports a delete for good it chose
        // itself (an item past the bin's limit that `bin.rs` didn't catch). Nothing can
        // bring it back, so it's said loudly, for a report to carry.
        Ok(Recycled::Unnamed) => {
            log::error!(
                target: "trash",
                "the Shell reported {} recycled but named no Recycle Bin item: it may be deleted for good",
                cmdr_fs::log_private::LogPrivate::path(path)
            );
            None
        }
        Err(refusal) => {
            return Err(MutationError::TrashRefused {
                reason: refusal.kind(),
                detail: refusal.to_string(),
            });
        }
    };
    // The Shell answers a skipped item with a success code too (`COPYENGINE_S_*`), so a
    // "recycled" item still in place wasn't: say so, ❌ never journal a trash that didn't happen.
    if fs::symlink_metadata(path).is_ok() {
        return Err(MutationError::TrashRefused {
            reason: TrashRefusalKind::Other,
            detail: "the Shell reported the item recycled, but it's still in place".into(),
        });
    }
    Ok(landed)
}

/// `None`: the Recycle Bin is a Shell folder with no path a pane can list (its
/// `$Recycle.Bin\<SID>` folder shows only the `$R` and `$I` names).
pub fn trash_dir_for_path(_path: &Path) -> Option<PathBuf> {
    None
}

/// Deletes the `$I…` record beside a `$R…` item a restore renamed back out, so the
/// Recycle Bin stops listing it. A path that isn't a Recycle Bin item, or a record
/// already gone, is left alone.
pub(crate) fn forget_restored(in_trash: &Path) {
    let Some(record) = recycle_record_of(in_trash) else {
        return;
    };
    if let Err(e) = fs::remove_file(&record)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!(
            target: "trash",
            "couldn't remove the Recycle Bin record {}: {}",
            cmdr_fs::log_private::LogPrivate::path(&record),
            cmdr_fs::log_error::LogError(&cmdr_fs::error_detail::ErrorDetail::from_io(&e))
        );
    }
}

/// The plain Win32 spelling the Shell parses (`C:\Users\x`, `\\server\share\x`), from
/// an app path or a native one, or `None` when the plain form would name a different
/// file or can't be spelled at all.
fn shell_spelling(path: &Path) -> Option<PathBuf> {
    AppPathBuf::from_native_unicode(path).ok()?.to_native_plain().ok()
}

/// The `$I…` record of the Recycle Bin item `$R…` at `item`, when `item` sits in a
/// `$Recycle.Bin\<SID>\` folder: the same name with its second character flipped.
fn recycle_record_of(item: &Path) -> Option<PathBuf> {
    let name = item.file_name()?.to_str()?;
    let suffix = name.strip_prefix("$R")?;
    let sid_folder = item.parent()?;
    let bin = sid_folder.parent()?.file_name()?.to_str()?;
    if !bin.eq_ignore_ascii_case("$Recycle.Bin") {
        return None;
    }
    Some(sid_folder.join(format!("$I{suffix}")))
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
