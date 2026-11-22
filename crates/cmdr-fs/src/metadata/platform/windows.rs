//! Windows: dates are real (`std` sets birth, mtime, and atime in one `SetFileInformationByHandle`,
//! and a handle opened with backup semantics reaches a folder or a link's own entry), and so is
//! a FILE's read-only attribute as Locked: the removal lock a move asks about
//! (`removal_lock`), what `clear_blockers` clears, and what `add_flags` puts back where the move
//! landed it. Every other class answers "this destination can't hold it" until its Windows body
//! exists: the mode as the read-only attribute, xattrs as alternate data streams, the DACL, and
//! Hidden, and a copy carries no read-only yet (`flags_of` reads none). ❗
//! `std::fs::remove_file` deletes a read-only file on Windows (it clears the attribute on the
//! way), so the user's own delete and Overwrite refuse it up front as Locked
//! (`cmdr-operations`' `local_disk::locked_against_removal`). The windows-port plan's tasks 35
//! and 36 own the rest (`docs/specs/windows-port-plan.md` § "Windows build state").

use std::ffi::OsString;
use std::fs::{File, Metadata, OpenOptions};
use std::os::windows::fs::{FileTimesExt as _, OpenOptionsExt as _};
use std::path::Path;
use std::time::SystemTime;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_WRITE_ATTRIBUTES,
};

use super::super::{
    ClassOutcome, LandedMetadata, MetadataCaps, MetadataClasses, ParentOpening, RemovalLock, StagedItem, StagedTimes,
    Unblock, UserFlags, Xattr,
};
use crate::log_private::LogPrivate;
use crate::os_error::OsErrorClass;

/// No ACL is read yet (the DACL is task 36's), so no code path can hold one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acl(std::convert::Infallible);

pub(crate) fn mode_of(_meta: &Metadata) -> Option<u32> {
    None
}

pub(crate) fn write_mode_at(_path: &Path, _mode: u32) -> ClassOutcome {
    ClassOutcome::NotApplicable
}

pub(crate) fn write_mode_staged(_item: StagedItem<'_>, _mode: u32) -> ClassOutcome {
    ClassOutcome::NotApplicable
}

/// Windows has no umask; the fallback only shapes the modes no Windows arm writes.
pub(crate) fn probe_umask() -> u32 {
    super::super::mode::FALLBACK_UMASK
}

/// No xattr is carried until alternate data streams back them (task 35).
pub(crate) fn carries_xattr(_name: &OsString) -> bool {
    false
}

pub(crate) fn local_xattrs(_file: &File) -> Vec<Xattr> {
    Vec::new()
}

pub(crate) fn local_xattr_names(_file: &File) -> Vec<Xattr> {
    Vec::new()
}

pub(crate) fn xattrs_at(_path: &Path) -> Vec<Xattr> {
    Vec::new()
}

pub(crate) fn write_xattrs(_item: StagedItem<'_>, xattrs: &[Xattr]) -> ClassOutcome {
    if xattrs.is_empty() {
        ClassOutcome::Absent
    } else {
        ClassOutcome::NotApplicable
    }
}

pub(crate) fn read_acl(_path: &Path) -> Option<Acl> {
    None
}

pub(crate) fn write_acl(_path: &Path, acl: &Acl) -> ClassOutcome {
    match acl.0 {}
}

pub(crate) fn flags_of(_meta: &Metadata) -> UserFlags {
    UserFlags::NONE
}

/// Only Locked has a Windows body: the read-only attribute on a FILE. It goes back on where a
/// move put an item it unlocked (`apply_landed`); a folder's read-only bit isn't a lock, so a
/// folder answers `NotApplicable`, as does every other flag.
pub(crate) fn add_flags(path: &Path, flags: UserFlags) -> ClassOutcome {
    if !flags.contains(UserFlags::LOCKED) {
        return ClassOutcome::NotApplicable;
    }
    let native = native_spelling(path);
    let path = native.as_deref().unwrap_or(path);
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => match set_read_only(path, meta.permissions(), true) {
            Ok(()) => ClassOutcome::Kept,
            Err(e) => {
                log::debug!(target: "metadata", "couldn't set read-only on {}: {e}", LogPrivate::path(path));
                ClassOutcome::Refused
            }
        },
        _ => ClassOutcome::NotApplicable,
    }
}

/// Clears a FILE's read-only attribute, Windows' Locked (`removal_lock`), and hands back
/// `LOCKED` so the caller puts it back where the item lands. Never follows a link (a link's
/// own entry isn't a file here).
pub(crate) fn clear_blockers(path: &Path) -> Unblock {
    super::super::note_helper_call();
    let native = native_spelling(path);
    let path = native.as_deref().unwrap_or(path);
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Unblock::NothingToClear;
    };
    if !is_locked_file(&meta) {
        return Unblock::NothingToClear;
    }
    match set_read_only(path, meta.permissions(), false) {
        Ok(()) => Unblock::Cleared(LandedMetadata::default().with_flags(UserFlags::LOCKED)),
        Err(e) => {
            log::debug!(target: "metadata", "couldn't clear read-only on {}: {e}", LogPrivate::path(path));
            Unblock::Refused
        }
    }
}

/// The read-only attribute on a FILE is Windows' Locked (Explorer's "Read-only" box), so a
/// move asks before carrying one off, as it asks for Finder's Locked. ❗ It's a consent rule,
/// not an OS refusal: Windows renames a read-only file, and `std::fs::remove_file` deletes
/// one (verified on Windows 10 22H2 with Rust 1.100, 2026-11-21). ❗ A folder's read-only bit
/// isn't a lock: Explorer sets it to mark a customized folder (`desktop.ini`), and nothing
/// honors it as one. Windows has no system-locked kind a process can't clear.
pub(crate) fn removal_lock(path: &Path) -> RemovalLock {
    let native = native_spelling(path);
    match std::fs::symlink_metadata(native.as_deref().unwrap_or(path)) {
        Ok(meta) if is_locked_file(&meta) => RemovalLock::Locked,
        _ => RemovalLock::Free,
    }
}

fn is_locked_file(meta: &Metadata) -> bool {
    meta.is_file() && meta.permissions().readonly()
}

/// `path` in its verbatim spelling, so a path past `MAX_PATH` or in the app spelling (`D:/x`,
/// what a volume walk carries) reaches the item itself. `None`: `path` as given.
fn native_spelling(path: &Path) -> Option<std::path::PathBuf> {
    crate::app_path::AppPathBuf::from_native_unicode(path)
        .ok()
        .and_then(|app| app.to_native().ok())
}

fn set_read_only(path: &Path, mut permissions: std::fs::Permissions, read_only: bool) -> std::io::Result<()> {
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "on Windows this only sets or clears the read-only attribute"
    )]
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions)
}

pub(crate) fn open_parent_folder(_child: &Path) -> ParentOpening {
    ParentOpening::NothingToOpen
}

/// Only the dates are kept so far; the birth date is reported off the open's metadata.
pub const LOCAL_DISK_CAPS: MetadataCaps = MetadataCaps {
    stores: MetadataClasses::CREATED,
    reports: MetadataClasses::CREATED,
    folder_stores: MetadataClasses::CREATED,
    folder_dates: true,
    staged_flags: UserFlags::NONE,
};

/// NTFS, ReFS, and exFAT keep a settable birth date.
pub(crate) const CREATED_SETTABLE: bool = true;

/// A staged file through its own handle; a folder or a link through a handle that asks only to
/// write attributes, with backup semantics (the only way to open a folder) and, for a link, the
/// reparse point itself rather than its target.
pub(crate) fn set_dates(item: StagedItem<'_>, created: Option<SystemTime>, times: StagedTimes) -> std::io::Result<()> {
    let mut file_times = times.file_times();
    if let Some(created) = created {
        file_times = file_times.set_created(created);
    }
    let (path, flags) = match item {
        StagedItem::File(file) => return file.set_times(file_times),
        StagedItem::Path(path) => (path, FILE_FLAG_BACKUP_SEMANTICS),
        StagedItem::Link(path) => (path, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT),
    };
    OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(flags)
        .open(path)?
        .set_times(file_times)
}

/// `ERROR_NOT_SUPPORTED` means the destination can't hold the class at all.
pub(crate) fn is_unsupported(e: &std::io::Error) -> bool {
    OsErrorClass::of_io(e) == Some(OsErrorClass::Unsupported)
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
