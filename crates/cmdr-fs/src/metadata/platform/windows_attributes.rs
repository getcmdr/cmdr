//! The Windows file attributes a copy carries, as [`UserFlags`]: the read-only attribute on a
//! FILE is Locked (Explorer's "Read-only" box, the lock a move asks about), and the hidden
//! attribute is Hidden on a file or a folder. Pure arithmetic over the attribute word, so every
//! OS's test run pins it; `windows.rs` is its one production caller.
//!
//! ❗ A folder's read-only bit is never Locked: Explorer sets it to mark a customized folder
//! (`desktop.ini`), and nothing honors it as a lock. So a folder neither reports nor takes it.

use super::super::UserFlags;

/// `FILE_ATTRIBUTE_READONLY`.
pub(crate) const READONLY: u32 = 0x1;
/// `FILE_ATTRIBUTE_HIDDEN`.
pub(crate) const HIDDEN: u32 = 0x2;
/// `FILE_ATTRIBUTE_NORMAL`: what `SetFileAttributesW` takes for "no attribute at all".
pub(crate) const NORMAL: u32 = 0x80;

/// The attributes `SetFileAttributesW` sets; the rest of the word (directory, reparse point,
/// compressed, encrypted, sparse, and the cloud placeholder bits) is the filesystem's, so a
/// write keeps it out.
const SETTABLE: u32 = READONLY
    | HIDDEN
    | 0x4 // SYSTEM
    | 0x20 // ARCHIVE
    | 0x100 // TEMPORARY
    | 0x1000 // OFFLINE
    | 0x2000; // NOT_CONTENT_INDEXED

/// The flags an item with `attributes` wears. `is_file`: a regular file, never a folder or a
/// link.
pub(crate) fn flags_of_attributes(attributes: u32, is_file: bool) -> UserFlags {
    let locked = if is_file && attributes & READONLY != 0 {
        UserFlags::LOCKED
    } else {
        UserFlags::NONE
    };
    let hidden = if attributes & HIDDEN != 0 {
        UserFlags::HIDDEN
    } else {
        UserFlags::NONE
    };
    locked | hidden
}

/// The flags of `flags` an item can take here: Hidden on anything, Locked on a file.
/// Append-only and no-dump have no Windows attribute.
pub(crate) fn settable_flags(flags: UserFlags, is_file: bool) -> UserFlags {
    let locked = if is_file { UserFlags::LOCKED } else { UserFlags::NONE };
    flags.intersection(UserFlags::HIDDEN | locked)
}

/// The attribute word `SetFileAttributesW` takes to ADD `flags` to an item now wearing
/// `attributes`: what it has, plus each flag's bit, minus what the call can't set, and
/// [`NORMAL`] in place of an empty word.
pub(crate) fn attributes_adding(attributes: u32, flags: UserFlags, is_file: bool) -> u32 {
    let flags = settable_flags(flags, is_file);
    let mut word = attributes & SETTABLE;
    if flags.contains(UserFlags::LOCKED) {
        word |= READONLY;
    }
    if flags.contains(UserFlags::HIDDEN) {
        word |= HIDDEN;
    }
    if word == 0 { NORMAL } else { word }
}

#[cfg(test)]
#[path = "windows_attributes_tests.rs"]
mod tests;
