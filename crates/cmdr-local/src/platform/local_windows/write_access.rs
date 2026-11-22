//! Whether a write into a folder would be taken (`Volume::write_access_at`), the Windows twin of
//! the POSIX `statvfs` plus `access(W_OK)` pair: the volume's read-only flag
//! (`GetVolumeInformationByHandleW`), then whether the folder's DACL grants this process's token
//! `FILE_ADD_FILE` (the right to make an entry in it), asked of `AccessCheck`. What the two
//! answers mean together: `super::super::write_verdict`.
//!
//! **Decision**: `AccessCheck` on the folder's security descriptor, ❌ not an open asking for
//! `FILE_ADD_FILE`. **Why**: a folder opens only with backup semantics, and a token with
//! `SeBackupPrivilege` ENABLED (a service, an elevated CI runner) is granted that open whatever
//! the DACL says, while the copy's own writes (no backup semantics) are refused: the open
//! answered `Writable` for a folder whose DACL denied everyone `FILE_ADD_FILE` on the hosted
//! runner (GitHub's `windows-latest`, PR #1374's first run, 2026-11-22). `AccessCheck` reads the
//! DACL alone, as a plain create does. A folder's read-only attribute isn't consulted: it's
//! Explorer's customization marker, and NTFS doesn't enforce it.

use std::fs::File;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::Path;

use cmdr_fs::volume::WriteAccess;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};

use super::super::write_verdict::write_verdict;

/// `FILE_READ_ONLY_VOLUME` in `GetVolumeInformationByHandleW`'s flags.
const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;

/// The verdict for the nearest folder at or above `path` (the OS's path) that exists, since a
/// copy creates the rest. `Unknown` when none can be found or read.
pub(crate) fn write_access_at(path: &Path) -> WriteAccess {
    let Some(folder) = path.ancestors().find(|ancestor| ancestor.is_dir()) else {
        return WriteAccess::Unknown;
    };
    let read_only_volume = volume_flags(folder).is_some_and(|flags| flags & FILE_READ_ONLY_VOLUME != 0);
    write_verdict(read_only_volume, may_add_file(folder))
}

/// The filesystem flags of the volume `folder` sits on, or `None` when Windows can't say.
fn volume_flags(folder: &Path) -> Option<u32> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetVolumeInformationByHandleW,
    };
    // Attributes only, every share mode, so another program's handle never makes this refuse.
    let handle: File = std::fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(folder)
        .ok()?;
    let mut flags = 0u32;
    // SAFETY: `handle` is open for the call; every out pointer but `flags` is null with a zero
    // length (not wanted), and `flags` is a live `u32`.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut flags,
            std::ptr::null_mut(),
            0,
        )
    };
    (ok != 0).then_some(flags)
}

/// A token handle closed on drop.
struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a token handle this process opened and nothing else closes.
        unsafe { CloseHandle(self.0) };
    }
}

/// Whether the DACL of `folder` grants this process's token `FILE_ADD_FILE`, or `None` when the
/// descriptor or the token can't be read.
fn may_add_file(folder: &Path) -> Option<bool> {
    use windows_sys::Win32::Security::{
        AccessCheck, DACL_SECURITY_INFORMATION, DuplicateToken, GENERIC_MAPPING, GROUP_SECURITY_INFORMATION,
        GetFileSecurityW, MapGenericMask, OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, SecurityImpersonation,
        TOKEN_DUPLICATE, TOKEN_QUERY,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ADD_FILE, FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let wide: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
    let wanted = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut needed = 0u32;
    // SAFETY: `wide` is NUL-terminated; a null buffer of length 0 asks only for the size.
    unsafe { GetFileSecurityW(wide.as_ptr(), wanted, std::ptr::null_mut(), 0, &raw mut needed) };
    if needed == 0 {
        return None;
    }
    // `u64`s, so the descriptor lands 8-byte aligned, as `SECURITY_DESCRIPTOR` wants.
    let mut descriptor = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: `descriptor` holds at least `needed` bytes, the length passed.
    let read = unsafe {
        GetFileSecurityW(
            wide.as_ptr(),
            wanted,
            descriptor.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        )
    };
    if read == 0 {
        return None;
    }

    let mut process_token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo handle needs no closing; `process_token` receives a handle we own.
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &raw mut process_token,
        )
    } == 0
    {
        return None;
    }
    let process_token = Token(process_token);
    let mut impersonation: HANDLE = std::ptr::null_mut();
    // SAFETY: `process_token` is open with `TOKEN_DUPLICATE`; `impersonation` receives a handle
    // we own. `AccessCheck` takes an impersonation token, never a primary one.
    if unsafe { DuplicateToken(process_token.0, SecurityImpersonation, &raw mut impersonation) } == 0 {
        return None;
    }
    let impersonation = Token(impersonation);

    let mapping = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ,
        GenericWrite: FILE_GENERIC_WRITE,
        GenericExecute: FILE_GENERIC_EXECUTE,
        GenericAll: FILE_ALL_ACCESS,
    };
    let mut desired = FILE_ADD_FILE;
    // SAFETY: both pointers are live locals.
    unsafe { MapGenericMask(&raw mut desired, &raw const mapping) };
    // SAFETY: plain old data.
    let mut privileges: PRIVILEGE_SET = unsafe { std::mem::zeroed() };
    let mut privileges_len = u32::try_from(size_of::<PRIVILEGE_SET>()).expect("a small struct");
    let mut granted = 0u32;
    let mut status = 0;
    // SAFETY: `descriptor` is the self-relative descriptor `GetFileSecurityW` wrote, the token is
    // an open impersonation token with `TOKEN_QUERY`, and every out pointer is a live local
    // (`privileges` with its exact size).
    let checked = unsafe {
        AccessCheck(
            descriptor.as_mut_ptr().cast(),
            impersonation.0,
            desired,
            &raw const mapping,
            &raw mut privileges,
            &raw mut privileges_len,
            &raw mut granted,
            &raw mut status,
        )
    };
    (checked != 0).then_some(status != 0)
}
