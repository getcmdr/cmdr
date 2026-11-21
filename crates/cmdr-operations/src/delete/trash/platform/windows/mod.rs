//! Windows: the Recycle Bin, through the Shell's `IFileOperation` (Microsoft's
//! replacement for `SHFileOperationW` since Vista), one item per call.
//!
//! ❗ The Shell DELETES an item it can't recycle (a share, a removable or `subst`
//! drive, a bin that's full or turned off, an item too big for it), silently under
//! `FOF_NO_UI`. So the progress sink vetoes every delete the Shell announces without
//! `TSF_DELETE_RECYCLE_IF_POSSIBLE` (`shell.rs`), and that refusal is
//! `NoTrashForVolume`: the item stays, and the UI offers the permanent delete it
//! asks consent for. A drive that isn't a fixed disk refuses before the Shell is even
//! asked ([`on_a_fixed_drive`]), so a share or a stick has two walls. ❌ Never pass
//! `FOF_WANTNUKEWARNING` instead: it shows the Shell's own dialog.
//!
//! The in-trash location is the Shell's `$R…` item under
//! `<drive>\$Recycle.Bin\<SID>\`, returned in the app spelling, so the trash undo
//! renames it back; [`forget_restored`] then deletes its `$I…` record, without
//! which the Recycle Bin would list an entry whose item is gone.

mod shell;

use std::fs;
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::volume::path_grammar::host_app_path;

use super::super::super::super::mutation_error::MutationError;
use super::super::super::super::types::TrashRefusalKind;

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
    // A second wall before the sink's veto: a drive that isn't a fixed disk (a share, a
    // USB stick, a disc) never has a Recycle Bin, so it refuses without asking the Shell.
    if !on_a_fixed_drive(&plain) {
        return Err(MutationError::TrashRefused {
            reason: TrashRefusalKind::NoTrashForVolume,
            detail: "this drive has no Recycle Bin (not a fixed disk)".into(),
        });
    }
    let landed = match shell::recycle(&plain) {
        Ok(Recycled::InBin(landed)) => Some(host_app_path(landed)),
        // Recycled, but the Shell named no item for it: trashed, with no restore location.
        Ok(Recycled::Unnamed) => None,
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

/// Whether `plain` sits on a fixed disk (`GetDriveTypeW`'s `DRIVE_FIXED`), the only kind
/// of drive Windows keeps a Recycle Bin on. A path whose volume Windows can't name answers
/// `true`: the sink's veto still guards it.
fn on_a_fixed_drive(plain: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};

    /// `GetDriveTypeW`'s `DRIVE_FIXED` (`WindowsProgramming`'s constant, a feature this crate
    /// doesn't otherwise need).
    const DRIVE_FIXED: u32 = 3;

    let wide: Vec<u16> = plain.as_os_str().encode_wide().chain(Some(0)).collect();
    // The volume root is never longer than the path itself (plus its trailing `\`).
    let mut root = vec![0u16; wide.len().max(261) + 1];
    let Ok(root_len) = u32::try_from(root.len()) else {
        return true;
    };
    // SAFETY: `wide` is NUL-terminated and `root` holds `root_len` units.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root_len) } == 0 {
        return true;
    }
    // SAFETY: `root` is the NUL-terminated root `GetVolumePathNameW` wrote.
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_FIXED }
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
