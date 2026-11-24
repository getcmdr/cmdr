//! Whether a place may be asked for its own icon: the Shell reads a program's or a shortcut's
//! file to draw it, so a place on a share, on an optical disc, or held only in the cloud is
//! never asked. Two cheap questions, neither of which reaches past this machine: the drive
//! letter's kind, then the file's attributes on a local drive.

use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, Prefix};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN, GetDriveTypeW,
    GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
};

/// `GetDriveTypeW`'s answers for a drive this machine holds (`WindowsProgramming`'s constants,
/// a feature the app doesn't otherwise need).
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_RAMDISK: u32 = 6;

/// The attributes of a cloud file whose bytes aren't here (a OneDrive or other Cloud Files
/// placeholder, an HSM-offloaded file): opening it starts a download.
const NOT_HERE: u32 = FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

/// Whether native `path` sits on a local drive and holds its bytes here. A UNC path
/// (`\\server\share`) answers no without a syscall; a mapped drive answers no from its
/// letter's kind, which Windows keeps locally; a path that doesn't exist (inside an archive,
/// gone since the listing) answers no.
pub(super) fn may_read(path: &Path) -> bool {
    on_local_drive(path) && holds_its_bytes(path)
}

fn on_local_drive(path: &Path) -> bool {
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return false;
    };
    let (Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)) = prefix.kind() else {
        return false;
    };
    let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
    // SAFETY: `root` is a NUL-terminated drive root (`C:\`).
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    matches!(kind, DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_RAMDISK)
}

fn holds_its_bytes(path: &Path) -> bool {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated. Reading attributes opens no data stream, so it
    // starts no cloud download.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    attributes != INVALID_FILE_ATTRIBUTES && attributes & NOT_HERE == 0
}
