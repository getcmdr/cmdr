//! Whether a place may be asked for its own icon: the Shell reads a program's or a shortcut's
//! file to draw it, so a place on a share, on an optical disc, behind a link, or held only in
//! the cloud is never asked. Three cheap questions on this machine: the path's shape, the drive
//! letter's kind, and the place's attributes (plus, for a link, its tag).
//!
//! What holds: no UNC path and no network or optical drive letter is asked, and neither is a
//! place that is itself a symlink or junction. What doesn't: a junction in a PARENT folder
//! (`C:\x` pointing at `\\nas\share`, then `C:\x\tool.exe`) passes, since catching it would
//! cost a check per ancestor; `GetFileAttributesW` on such a place then reaches the share.

use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, Prefix};

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT, FindClose, FindFirstFileW, GetDriveTypeW, GetFileAttributesW,
    INVALID_FILE_ATTRIBUTES, WIN32_FIND_DATAW,
};

/// `GetDriveTypeW`'s answers for a drive this machine holds (`WindowsProgramming`'s constants,
/// a feature the app doesn't otherwise need).
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_RAMDISK: u32 = 6;

/// The attributes of a cloud file whose bytes aren't here (a OneDrive or other Cloud Files
/// placeholder, an HSM-offloaded file): opening it starts a download.
const NOT_HERE: u32 = FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

/// The reparse-tag bit Windows sets on a tag that names another place (a symlink, a junction).
/// Cloud Files and app-execution aliases don't carry it, so a hydrated OneDrive file still asks.
const NAME_SURROGATE: u32 = 0x2000_0000;

/// Whether native `path` sits on a local drive, isn't a link to elsewhere, and holds its bytes
/// here. A UNC path (`\\server\share`) and a verbatim one (`\\?\C:\…`, past `MAX_PATH`, which
/// the Shell won't parse) answer no by shape; a mapped drive answers no from its letter's kind,
/// which Windows keeps locally; a path that doesn't exist (inside an archive, gone since the
/// listing) answers no.
pub(super) fn may_read(path: &Path) -> bool {
    local_attributes(path).is_some()
}

/// [`may_read`] with the place's attributes, read once: `Some` only when it may be asked.
pub(super) fn local_attributes(path: &Path) -> Option<u32> {
    if !on_local_drive(path) {
        return None;
    }
    attributes_if_here(path)
}

fn on_local_drive(path: &Path) -> bool {
    drive_type(path).is_some_and(|kind| matches!(kind, DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_RAMDISK))
}

/// Whether native `path` sits on a fixed or RAM drive: local, and not a stick anyone could
/// hand over (where Cmdr reads no `desktop.ini`).
pub(super) fn on_fixed_drive(path: &Path) -> bool {
    drive_type(path).is_some_and(|kind| matches!(kind, DRIVE_FIXED | DRIVE_RAMDISK))
}

/// `GetDriveTypeW` of the drive letter `path` starts with; `None` for any other shape (UNC,
/// verbatim, relative), which no call is made for.
fn drive_type(path: &Path) -> Option<u32> {
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    let Prefix::Disk(letter) = prefix.kind() else {
        return None;
    };
    let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
    // SAFETY: `root` is a NUL-terminated drive root (`C:\`).
    Some(unsafe { GetDriveTypeW(root.as_ptr()) })
}

/// The raw attributes of `path` itself (a link is not followed), `None` when unreadable.
pub(super) fn raw_attributes(path: &Path) -> Option<u32> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated; reading attributes opens no stream and follows no link.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    (attributes != INVALID_FILE_ATTRIBUTES).then_some(attributes)
}

/// Whether the place's attributes say a cloud provider holds its bytes elsewhere.
pub(super) fn bytes_elsewhere(attributes: u32) -> bool {
    attributes & NOT_HERE != 0
}

/// The place's attributes when it holds its bytes here and isn't a link to elsewhere.
fn attributes_if_here(path: &Path) -> Option<u32> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated. Reading attributes opens no data stream, so it
    // starts no cloud download.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES || attributes & NOT_HERE != 0 {
        return None;
    }
    (attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 || !names_another_place(&wide)).then_some(attributes)
}

/// Whether the reparse point at NUL-terminated `wide` is a symlink or junction, read from its
/// tag without following it. Unreadable counts as yes.
fn names_another_place(wide: &[u16]) -> bool {
    // SAFETY: an all-zero `WIN32_FIND_DATAW` is a valid value of this plain-data struct.
    let mut found: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    // SAFETY: `wide` is NUL-terminated and `found` is a live out-struct for the call.
    let handle = unsafe { FindFirstFileW(wide.as_ptr(), &raw mut found) };
    if handle == INVALID_HANDLE_VALUE {
        return true;
    }
    // SAFETY: `handle` came from the successful `FindFirstFileW` above.
    unsafe { FindClose(handle) };
    // For a reparse point, `dwReserved0` holds its tag.
    found.dwReserved0 & NAME_SURROGATE != 0
}
