//! Windows: dates are real (`std` sets birth, mtime, and atime in one `SetFileInformationByHandle`,
//! and a handle opened with backup semantics reaches a folder or a link's own entry). Every
//! other class answers "this destination can't hold it" until its Windows body exists: the mode
//! as the read-only attribute, xattrs as alternate data streams, the DACL, Hidden and Locked as
//! file attributes, and read-only as a removal blocker. ❗ `std::fs::remove_file` deletes a
//! read-only file on Windows (it clears the attribute on the way), so nothing here keeps one:
//! the user's own delete and Overwrite refuse it up front as Locked instead
//! (`cmdr-operations`' `local_disk::locked_against_removal`). The windows-port plan's tasks 35
//! and 36 own those bodies (`docs/specs/windows-port-plan.md` § "Windows build state").

use std::ffi::OsString;
use std::fs::{File, Metadata, OpenOptions};
use std::os::windows::fs::{FileTimesExt as _, OpenOptionsExt as _};
use std::path::Path;
use std::time::SystemTime;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_WRITE_ATTRIBUTES,
};

use super::super::{
    ClassOutcome, MetadataCaps, MetadataClasses, ParentOpening, RemovalLock, StagedItem, StagedTimes, Unblock,
    UserFlags, Xattr,
};
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

pub(crate) fn add_flags(_path: &Path, _flags: UserFlags) -> ClassOutcome {
    ClassOutcome::NotApplicable
}

pub(crate) fn clear_blockers(_path: &Path) -> Unblock {
    Unblock::NothingToClear
}

pub(crate) fn removal_lock(_path: &Path) -> RemovalLock {
    RemovalLock::Free
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
