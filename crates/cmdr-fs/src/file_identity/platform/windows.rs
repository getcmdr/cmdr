//! Windows keeps a node's id (volume serial plus file id) behind a handle, not in
//! `std::fs::Metadata` (`std`'s `MetadataExt::file_index` is unstable, and a listing's
//! metadata comes from `FindFirstFileW`, which carries none), so every `Metadata` answer here
//! is "don't know". The path readers [`file_id_at`] and [`device_at`] open a handle and ask it
//! (the node's volume serial and id; the volume's GUID or share, for [`device_at`]).
//!
//! - **The read**: `GetFileInformationByHandleEx(FileIdInfo)`, the 64-bit volume serial plus
//!   the 128-bit file id, which holds a ReFS id whole. On NTFS the id is the 64-bit file
//!   reference (record number plus a sequence number that moves on reuse), so a file deleted
//!   and replaced under its name reads as a different node.
//! - **The fallback**, only where the FILESYSTEM refuses that class (FAT and exFAT on older
//!   systems): `GetFileInformationByHandle`'s 32-bit serial plus 64-bit file index. The class
//!   a filesystem answers is fixed, so two reads of one file never mix the two spaces. Any
//!   other failure answers `None`, ❌ never a guess.
//! - **A zero id is no id**: some SMB servers answer zero for every file, which would make
//!   every pair "the same file". All ones is `FILE_INVALID_FILE_ID`.
//! - **The handle asks for `FILE_READ_ATTRIBUTES` only**, shares read, write, and delete, and
//!   opens folders (`FILE_FLAG_BACKUP_SEMANTICS`), so it never blocks or is blocked by
//!   another program holding the file.

use std::fs::{Metadata, OpenOptions};
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo,
    GetFileInformationByHandle, GetFileInformationByHandleEx, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    VOLUME_NAME_GUID,
};

use super::super::{DeviceId, FileId, Follow};

pub(crate) fn file_id(_metadata: &Metadata) -> Option<FileId> {
    None
}

pub(crate) fn file_id_at(path: &Path, follow: Follow) -> std::io::Result<Option<FileId>> {
    // An open that fails (missing, access denied) is the `Err`: the caller tells a missing
    // path from one it couldn't open by the error's kind.
    let file = open_for_id(path, follow)?;
    let handle: HANDLE = file.as_raw_handle();
    Ok(match id_info(handle) {
        Ok(info) => usable(info.VolumeSerialNumber, u128::from_le_bytes(info.FileId.Identifier)),
        Err(code) if refuses_the_class(code) => {
            file_index(handle).and_then(|info| usable(u64::from(info.dwVolumeSerialNumber), index_of(&info)))
        }
        Err(_) => None,
    })
}

/// The volume a path's node lives on, links followed: its mount manager GUID
/// (`\\?\Volume{…}\`, `GetFinalPathNameByHandleW` with `VOLUME_NAME_GUID`), or for a file on
/// a share, the share (`\\?\UNC\server\share`, compared without case). `None` for anything
/// else (a volume the mount manager doesn't know, an unreadable path).
///
/// ❗ Never the volume serial [`file_id_at`] reads: a share shows the serial of the server's
/// disk under it, so a local folder and `\\localhost\C$`, or two shares on one server disk,
/// read as one device while a rename between them fails `ERROR_NOT_SAME_DEVICE` (17); and a
/// cloned disk keeps its source's serial. (Verified on Windows 10 22H2, `ssh mami`,
/// PowerShell P/Invoke, 2026-11-21: `C:\…` and `\\localhost\C$\…` both read serial
/// `7C409CAE409C709E`, `MoveFileExW` between them failed with 17, and `VOLUME_NAME_GUID`
/// answered `ERROR_PATH_NOT_FOUND` (3) on the share.) So this device compares only with
/// another [`device_at`], ❌ never with a [`FileId`]'s.
pub(crate) fn device_at(path: &Path) -> Option<DeviceId> {
    let file = open_for_id(path, Follow::Links).ok()?;
    let handle: HANDLE = file.as_raw_handle();
    let key = match final_path(handle, VOLUME_NAME_GUID) {
        Some(guid_path) => volume_key(&guid_path),
        None => share_key(&final_path(handle, VOLUME_NAME_DOS)?),
    };
    key.map(DeviceId::from_raw)
}

/// The path `handle` names, its volume spelled per `volume_flag`, or `None` when Windows
/// won't spell it that way.
fn final_path(handle: HANDLE, volume_flag: u32) -> Option<String> {
    let mut buffer = vec![0u16; 512];
    for _ in 0..3 {
        let len = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: `handle` is an open file handle the caller's `File` owns for this whole
        // call, and `buffer` holds `len` units; an answer of `len` or more is the length it
        // needs (with its NUL), with nothing written.
        let written =
            unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), len, FILE_NAME_NORMALIZED | volume_flag) }
                as usize;
        if written == 0 {
            return None;
        }
        if written < buffer.len() {
            return Some(String::from_utf16_lossy(&buffer[..written]));
        }
        buffer = vec![0u16; written + 1];
    }
    None
}

/// The key of the volume a `VOLUME_NAME_GUID` path names (`\\?\Volume{…}\rest`).
fn volume_key(guid_path: &str) -> Option<u64> {
    let rest = guid_path.strip_prefix(r"\\?\")?;
    let volume = rest.split('\\').next()?.to_ascii_lowercase();
    volume
        .starts_with("volume{")
        .then(|| fnv1a(format!("volume:{volume}").as_bytes()))
}

/// The key of the share a `VOLUME_NAME_DOS` path sits on (`\\?\UNC\server\share\rest`),
/// `None` for a path that isn't on one.
fn share_key(dos_path: &str) -> Option<u64> {
    let mut parts = dos_path.strip_prefix(r"\\?\UNC\")?.split('\\');
    let (server, share) = (parts.next()?, parts.next()?);
    (!server.is_empty() && !share.is_empty())
        .then(|| fnv1a(format!(r"share:{}\{}", server.to_lowercase(), share.to_lowercase()).as_bytes()))
}

/// FNV-1a over `bytes`: a stable 64-bit key, compared only within one session. Two keys
/// colliding would make a rename between two volumes refuse with `ERROR_NOT_SAME_DEVICE`,
/// never land somewhere wrong.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A handle that reads attributes only, shares everything, and opens folders, so it never
/// blocks or is blocked by another program holding the file.
fn open_for_id(path: &Path, follow: Follow) -> std::io::Result<std::fs::File> {
    let flags = match follow {
        Follow::Links => FILE_FLAG_BACKUP_SEMANTICS,
        Follow::NotLinks => FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    };
    OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(flags)
        .open(path)
}

/// `FileIdInfo`'s answer, or `Err` with the Win32 code when the call failed.
fn id_info(handle: HANDLE) -> Result<FILE_ID_INFO, Option<i32>> {
    let mut info = FILE_ID_INFO::default();
    let size = u32::try_from(size_of::<FILE_ID_INFO>()).expect("FILE_ID_INFO is 24 bytes");
    // SAFETY: `handle` is an open file handle the caller's `File` owns for this whole call,
    // and `info` is a `FILE_ID_INFO` of exactly `size` bytes, the buffer `FileIdInfo` fills.
    let ok = unsafe { GetFileInformationByHandleEx(handle, FileIdInfo, (&raw mut info).cast(), size) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().raw_os_error());
    }
    Ok(info)
}

/// The older handle information (32-bit serial, 64-bit file index), for a filesystem that
/// refuses `FileIdInfo`.
fn file_index(handle: HANDLE) -> Option<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is an open file handle the caller's `File` owns for this whole call,
    // and `info` is the `BY_HANDLE_FILE_INFORMATION` the call fills.
    let ok = unsafe { GetFileInformationByHandle(handle, &raw mut info) };
    (ok != 0).then_some(info)
}

/// The 64-bit file index the older call splits in two.
fn index_of(info: &BY_HANDLE_FILE_INFORMATION) -> u128 {
    u128::from((u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
}

/// Whether a failed `FileIdInfo` call means the filesystem doesn't offer that class, the one
/// failure the older index stands in for.
fn refuses_the_class(code: Option<i32>) -> bool {
    let code = code.and_then(|code| u32::try_from(code).ok());
    matches!(
        code,
        Some(ERROR_INVALID_PARAMETER | ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED)
    )
}

/// An id that names one node, or `None` for a zero or all-ones one.
fn usable(volume: u64, node: u128) -> Option<FileId> {
    let all_ones = node == u128::MAX || node == u128::from(u64::MAX);
    (node != 0 && !all_ones).then(|| FileId::from_parts(DeviceId::from_raw(volume), node))
}

pub(crate) fn device_of(_metadata: &Metadata) -> Option<DeviceId> {
    None
}

pub(crate) fn link_count(_metadata: &Metadata) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_or_invalid_id_is_no_id() {
        assert_eq!(usable(7, 0), None);
        assert_eq!(usable(7, u128::MAX), None);
        assert_eq!(usable(7, u128::from(u64::MAX)), None);
        assert!(usable(7, 42).is_some());
    }

    #[test]
    fn a_volume_is_keyed_by_its_guid_whatever_follows_or_its_case() {
        let root = volume_key(r"\\?\Volume{7ee27290-b54a-45d8-bcfd-9f3bd4318a54}\");
        assert!(root.is_some());
        assert_eq!(
            root,
            volume_key(r"\\?\Volume{7EE27290-B54A-45D8-BCFD-9F3BD4318A54}\Users\x")
        );
        assert_ne!(root, volume_key(r"\\?\Volume{caa3226d-b12e-447b-acbe-74d20a3f2a83}\"));
        assert_eq!(volume_key(r"\\?\C:\Users"), None);
    }

    /// A share is its own device, whichever disk is under it: `\\localhost\C$` shows `C:`'s
    /// serial, and a rename between the two fails `ERROR_NOT_SAME_DEVICE`.
    #[test]
    fn a_share_is_keyed_by_server_and_share_and_a_local_path_by_neither() {
        let share = share_key(r"\\?\UNC\testnas.local\Photos\2024");
        assert!(share.is_some());
        assert_eq!(share, share_key(r"\\?\UNC\TESTNAS.LOCAL\photos"));
        assert_ne!(share, share_key(r"\\?\UNC\testnas.local\Music\2024"));
        assert_ne!(share, volume_key(r"\\?\Volume{7ee27290-b54a-45d8-bcfd-9f3bd4318a54}\"));
        assert_eq!(share_key(r"\\?\C:\Users"), None);
        assert_eq!(share_key(r"\\?\UNC\testnas.local"), None);
    }
}
