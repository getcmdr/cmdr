//! Windows: dates are real (`std` sets birth, mtime, and atime in one `SetFileInformationByHandle`,
//! and a handle opened with backup semantics reaches a folder or a link's own entry), and so are
//! the two attributes a copy carries as user flags (`windows_attributes.rs`): a FILE's read-only
//! attribute as Locked, and the hidden attribute as Hidden. A copy reads them off its source
//! (`flags_of`) and puts them back where the item lands (`add_flags`, after the landing rename,
//! like every flag); Locked is also the removal lock a move asks about (`removal_lock`) and what
//! `clear_blockers` clears. The mode, xattrs (alternate data streams: `cmdr-local`'s
//! `DETAILS.md` § "The Windows sibling"), and the DACL answer "this destination can't hold it".
//! ❗ `std::fs::remove_file` deletes a read-only file on Windows (it clears the attribute on the
//! way), so the user's own delete and Overwrite refuse it up front as Locked (`cmdr-operations`'
//! `local_disk::locked_against_removal`).

use std::ffi::OsString;
use std::fs::{File, Metadata, OpenOptions};
use std::os::windows::fs::{FileTimesExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::Path;
use std::time::SystemTime;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_WRITE_ATTRIBUTES,
};

use super::super::{
    ClassOutcome, LandedMetadata, MetadataCaps, MetadataClasses, ParentOpening, RemovalLock, StagedItem, StagedTimes,
    Unblock, UserFlags, Xattr,
};
use super::windows_attributes::{attributes_adding, flags_of_attributes, settable_flags};
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

/// Locked (a FILE's read-only attribute) and Hidden, off the stat a copy already holds.
pub(crate) fn flags_of(meta: &Metadata) -> UserFlags {
    flags_of_attributes(meta.file_attributes(), meta.is_file())
}

/// Adds Hidden (on anything) and Locked (on a FILE, as its read-only attribute) to what the
/// item wears, in one `SetFileAttributesW`, never through a link. It runs after the landing
/// rename, where a move puts back what it unlocked and a copy puts what its source wore. An
/// item that can take none of `flags` (a folder asked for Locked; append-only, no-dump; a link)
/// answers `NotApplicable`.
pub(crate) fn add_flags(path: &Path, flags: UserFlags) -> ClassOutcome {
    let native = native_spelling(path);
    let path = native.as_deref().unwrap_or(path);
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_symlink() => meta,
        _ => return ClassOutcome::NotApplicable,
    };
    if settable_flags(flags, meta.is_file()).is_empty() {
        return ClassOutcome::NotApplicable;
    }
    let attributes = meta.file_attributes();
    let wanted = attributes_adding(attributes, flags, meta.is_file());
    if wanted == attributes {
        return ClassOutcome::Kept;
    }
    match set_attributes(path, wanted) {
        Ok(()) => ClassOutcome::Kept,
        Err(e) => {
            log::debug!(target: "metadata", "couldn't set the attributes on {}: {e}", LogPrivate::path(path));
            ClassOutcome::Refused
        }
    }
}

/// `SetFileAttributesW` on `path` (the verbatim spelling, so past `MAX_PATH` too).
fn set_attributes(path: &Path, attributes: u32) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call, which only reads it.
    if unsafe { SetFileAttributesW(wide.as_ptr(), attributes) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
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

/// The dates and the flags (Locked as a file's read-only attribute, and Hidden), each reported
/// off the open's metadata. The flags go on after the landing (`staged_flags` empty), as on
/// macOS: a read-only temp would refuse the replacing rename an Overwrite lands it with.
pub const LOCAL_DISK_CAPS: MetadataCaps = MetadataCaps {
    stores: MetadataClasses::CREATED.union(MetadataClasses::FLAGS),
    reports: MetadataClasses::CREATED.union(MetadataClasses::FLAGS),
    folder_stores: MetadataClasses::CREATED.union(MetadataClasses::FLAGS),
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
