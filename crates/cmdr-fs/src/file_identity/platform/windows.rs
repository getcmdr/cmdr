//! Windows keeps a node's id (volume serial plus file id) behind a handle, not in
//! `std::fs::Metadata` (`std`'s `MetadataExt::file_index` is unstable, and a listing's
//! metadata comes from `FindFirstFileW`, which carries none), so every `Metadata` answer here
//! is "don't know". The path reader [`file_id_at`] opens a handle and asks it.
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
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandle,
    GetFileInformationByHandleEx,
};

use super::super::{DeviceId, FileId, Follow};

pub(crate) fn file_id(_metadata: &Metadata) -> Option<FileId> {
    None
}

pub(crate) fn file_id_at(path: &Path, follow: Follow) -> Option<FileId> {
    let flags = match follow {
        Follow::Links => FILE_FLAG_BACKUP_SEMANTICS,
        Follow::NotLinks => FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(flags)
        .open(path)
        .ok()?;
    let handle: HANDLE = file.as_raw_handle();
    match id_info(handle) {
        Ok(id) => id,
        Err(code) if refuses_the_class(code) => file_index(handle),
        Err(_) => None,
    }
}

/// `FileIdInfo`'s answer: `Ok(None)` for a zero or invalid id, `Err` with the Win32 code
/// when the call failed.
fn id_info(handle: HANDLE) -> Result<Option<FileId>, Option<i32>> {
    let mut info = FILE_ID_INFO::default();
    let size = u32::try_from(size_of::<FILE_ID_INFO>()).expect("FILE_ID_INFO is 24 bytes");
    // SAFETY: `handle` is an open file handle `file_id_at`'s `File` owns for this whole
    // call, and `info` is a `FILE_ID_INFO` of exactly `size` bytes, the buffer `FileIdInfo`
    // fills.
    let ok = unsafe { GetFileInformationByHandleEx(handle, FileIdInfo, (&raw mut info).cast(), size) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().raw_os_error());
    }
    Ok(usable(
        info.VolumeSerialNumber,
        u128::from_le_bytes(info.FileId.Identifier),
    ))
}

/// The older 64-bit file index, for a filesystem that refuses `FileIdInfo`.
fn file_index(handle: HANDLE) -> Option<FileId> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is an open file handle `file_id_at`'s `File` owns for this whole
    // call, and `info` is the `BY_HANDLE_FILE_INFORMATION` the call fills.
    let ok = unsafe { GetFileInformationByHandle(handle, &raw mut info) };
    if ok == 0 {
        return None;
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    usable(u64::from(info.dwVolumeSerialNumber), u128::from(index))
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
}
