//! Windows: `CreateFileW` with `CREATE_NEW` and a security descriptor built from
//! `O:<me>D:P(A;;FA;;;<me>)`, where `<me>` is the process token's user SID. So the file is born
//! owned by this user, with a protected DACL (`P`: nothing inherits from the folder) holding one
//! entry: full access for this user. `std`'s `OpenOptions` can't pass a descriptor, which is why
//! this calls `CreateFileW` itself. The rename that finishes an atomic write keeps the explicit
//! DACL, and the protection keeps the new folder's inheritable entries out.
//!
//! No `SYSTEM` or `Administrators` entry, on purpose: nothing Cmdr runs needs one, and an
//! administrator who wants the file has to take ownership first, which is audited and visible,
//! where an inherited entry would let them read it silently. `DETAILS.md` § "Private files".

use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, GetSecurityDescriptorControl,
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// `ACCESS_ALLOWED_ACE_TYPE` (`winnt.h`), spelled here so the seam needs no `SystemServices`.
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
/// `ACCESS_DENIED_ACE_TYPE` (`winnt.h`), which shares the allowed entry's layout.
const ACCESS_DENIED_ACE_TYPE: u8 = 1;

/// A `CREATE_NEW` open carrying the owner-only descriptor. `FILE_FLAG_OPEN_REPARSE_POINT` makes
/// a link at `path` count as the thing that's there, so the create fails on it rather than
/// following it.
pub(crate) fn create_new(path: &Path) -> io::Result<File> {
    let me = current_user_sid()?;
    let sddl = wide(OsStr::new(&format!("O:{me}D:P(A;;FA;;;{me})")))?;
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `sddl` is a NUL-terminated UTF-16 string that outlives the call, and `descriptor`
    // is a valid out pointer; the size out pointer may be null.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    let descriptor = LocalMemory(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let path = wide(path.as_os_str())?;
    // SAFETY: `path` is a NUL-terminated UTF-16 string, and `attributes` points at a descriptor
    // that stays alive (`descriptor`) until this returns; no template handle.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a valid file handle this call just opened and nothing else owns.
    Ok(unsafe { File::from_raw_handle(handle) })
}

/// A regular file owned by this user, with a protected DACL holding exactly one entry: full
/// access for this user.
pub(crate) fn is_private_to_current_user(path: &Path) -> io::Result<bool> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Ok(false);
    }
    let me = current_user_sid()?;
    let security = file_security(path)?;
    let only_me = AccessEntry {
        kind: ACCESS_ALLOWED_ACE_TYPE,
        flags: 0,
        mask: FILE_ALL_ACCESS,
        sid: Some(me.clone()),
    };
    Ok(security.owner == me && security.dacl_protected && security.entries == Some(vec![only_me]))
}

/// What a file's security descriptor says about who can open it.
#[derive(Debug)]
pub(crate) struct FileSecurity {
    /// The owner's SID, `S-1-…`.
    pub(crate) owner: String,
    /// Whether the DACL is protected from the folder's inheritable entries.
    pub(crate) dacl_protected: bool,
    /// The DACL's entries in order, or `None` for a NULL DACL, which lets everyone in.
    pub(crate) entries: Option<Vec<AccessEntry>>,
}

/// One entry of a DACL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AccessEntry {
    /// The ACE type: 0 allows, 1 denies, anything else is an object or callback entry.
    pub(crate) kind: u8,
    /// The ACE flags (inheritance, among them).
    pub(crate) flags: u8,
    /// The access mask, for an allow or deny entry.
    pub(crate) mask: u32,
    /// The trustee's SID, for an allow or deny entry.
    pub(crate) sid: Option<String>,
}

/// Reads the owner and DACL of the file at `path`.
pub(crate) fn file_security(path: &Path) -> io::Result<FileSecurity> {
    let path = wide(path.as_os_str())?;
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `path` is NUL-terminated; the out pointers are valid, and the group and SACL ones may
    // be null since their parts aren't asked for. `owner` and `dacl` point into `descriptor`.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let descriptor = LocalMemory(descriptor);

    let mut control = 0u16;
    let mut revision = 0u32;
    // SAFETY: `descriptor` is the valid descriptor just returned; both out pointers are valid.
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let entries = if dacl.is_null() {
        None
    } else {
        // SAFETY: a non-null `dacl` points at the ACL inside `descriptor`, alive in this scope.
        let count = unsafe { (*dacl).AceCount };
        let mut entries = Vec::with_capacity(usize::from(count));
        for index in 0..u32::from(count) {
            let mut ace: *mut c_void = ptr::null_mut();
            // SAFETY: `index` is below the ACL's own count, and `ace` is a valid out pointer.
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every ACE starts with an `ACE_HEADER`.
            let header = unsafe { *ace.cast::<ACE_HEADER>() };
            let entry = if matches!(header.AceType, ACCESS_ALLOWED_ACE_TYPE | ACCESS_DENIED_ACE_TYPE) {
                let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
                // SAFETY: allow and deny entries share `ACCESS_ALLOWED_ACE`'s layout, whose SID
                // starts at `SidStart`.
                let (mask, sid) = unsafe { ((*ace).Mask, ptr::addr_of_mut!((*ace).SidStart).cast::<c_void>()) };
                AccessEntry {
                    kind: header.AceType,
                    flags: header.AceFlags,
                    mask,
                    sid: Some(sid_string(sid)?),
                }
            } else {
                AccessEntry {
                    kind: header.AceType,
                    flags: header.AceFlags,
                    mask: 0,
                    sid: None,
                }
            };
            entries.push(entry);
        }
        Some(entries)
    };

    Ok(FileSecurity {
        owner: sid_string(owner)?,
        dacl_protected: control & SE_DACL_PROTECTED != 0,
        entries,
    })
}

/// The user SID of this process's token, `S-1-…`.
pub(crate) fn current_user_sid() -> io::Result<String> {
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo-handle from `GetCurrentProcess` needs no closing; `raw` is a valid out
    // pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is the token handle just opened, owned by nothing else.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let token = std::os::windows::io::AsRawHandle::as_raw_handle(&token);

    let mut needed = 0u32;
    // SAFETY: a size query: a null buffer of length 0, and a valid out pointer for the size. It
    // fails with `ERROR_INSUFFICIENT_BUFFER` by design, so only `needed` is read.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // `u64` words, so the buffer is aligned for `TOKEN_USER`'s pointer.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: `buffer` holds at least `needed` bytes, and the out pointer is valid.
    if unsafe { GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), needed, &mut needed) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call filled `buffer` with a `TOKEN_USER`, whose SID points into `buffer` too.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    sid_string(sid)
}

/// `sid` as `S-1-…`. Always the numeric form, never an SDDL alias like `BA`.
fn sid_string(sid: PSID) -> io::Result<String> {
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` is a valid SID the caller holds alive; `text` is a valid out pointer.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let text = LocalMemory(text.cast());
    let start = text.0.cast::<u16>();
    let mut len = 0;
    // SAFETY: the call returned a NUL-terminated UTF-16 string, alive until `text` drops.
    while unsafe { *start.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` units before the NUL are initialized.
    let units = unsafe { std::slice::from_raw_parts(start, len) };
    Ok(String::from_utf16_lossy(units))
}

/// `value` as a NUL-terminated UTF-16 string, refusing an interior NUL, which would cut the
/// string short and name something else.
fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut units: Vec<u16> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a NUL inside a path or descriptor",
        ));
    }
    units.push(0);
    Ok(units)
}

/// Memory a Win32 call allocated with `LocalAlloc`, freed on drop.
struct LocalMemory(*mut c_void);

impl Drop for LocalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a call documented to allocate with `LocalAlloc`, and
            // this is its only free.
            unsafe { LocalFree(self.0) };
        }
    }
}
