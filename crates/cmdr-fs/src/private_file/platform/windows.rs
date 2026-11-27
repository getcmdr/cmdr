//! Windows: `CreateFileW` with `CREATE_NEW` and a security descriptor built from
//! `O:<me>D:P(A;;FA;;;<me>)`, where `<me>` is the process token's user SID. So the file is born
//! owned by this user, with a protected DACL (`P`: nothing inherits from the folder) holding one
//! entry: full access for this user. `std`'s `OpenOptions` can't pass a descriptor, which is why
//! this calls `CreateFileW` itself. The rename that finishes an atomic write keeps the explicit
//! DACL, and the protection keeps the new folder's inheritable entries out.
//!
//! No `SYSTEM` or `Administrators` entry: nothing Cmdr runs needs one. That's no barrier to an
//! elevated admin, who reads through the backup privilege; the gain is against a loose or relocated
//! folder. `DETAILS.md` § "Private files".

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
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, GetLengthSid, GetSecurityDescriptorControl,
    GetTokenInformation, IsValidSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
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
    let wide_path = wide(path.as_os_str())?;
    // SAFETY: `wide_path` is a NUL-terminated UTF-16 string, and `attributes` points at a
    // descriptor that stays alive (`descriptor`) until this returns; no template handle.
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let err = io::Error::last_os_error();
        // A junction at the name (a folder reparse point) doesn't answer `ERROR_FILE_EXISTS`
        // (verified on the hosted Windows runner by the junction cell, run 45234643738). It's still
        // something at the name, so it reads as `AlreadyExists` like every other occupant, keeping
        // the OS error inside.
        if err.kind() != io::ErrorKind::AlreadyExists && std::fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, err));
        }
        return Err(err);
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
    let descriptor = SecurityDescriptor::of_file(path)?;
    let entries = match descriptor.dacl() {
        Some(dacl) => Some(dacl.entries()?),
        None => None,
    };
    Ok(FileSecurity {
        owner: sid_string(descriptor.owner)?,
        dacl_protected: descriptor.control()? & SE_DACL_PROTECTED != 0,
        entries,
    })
}

/// A file's security descriptor, as `GetNamedSecurityInfoW` allocated it. The owner SID and the
/// DACL point INTO that one block, so the DACL and its entries are handed out only as borrows of
/// this, and can't outlive the free in `memory`'s drop.
struct SecurityDescriptor {
    memory: LocalMemory,
    /// The owner SID, inside `memory`.
    owner: PSID,
    /// The DACL inside `memory`, or null for a NULL DACL.
    dacl: *const ACL,
}

impl SecurityDescriptor {
    /// The owner and DACL parts of the descriptor of the file at `path`.
    fn of_file(path: &Path) -> io::Result<Self> {
        let path = wide(path.as_os_str())?;
        let mut owner: PSID = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `path` is NUL-terminated; the out pointers are valid, and the group and SACL ones
        // may be null since their parts aren't asked for. On success `owner` and `dacl` point into
        // `descriptor` (`dacl` is null for a NULL DACL), which `memory` then owns.
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
        Ok(Self {
            memory: LocalMemory(descriptor),
            owner,
            dacl: dacl.cast_const(),
        })
    }

    /// The descriptor's control bits (`SE_DACL_PROTECTED`, among them).
    fn control(&self) -> io::Result<u16> {
        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: `memory` holds the valid descriptor `of_file` got; both out pointers are valid.
        if unsafe { GetSecurityDescriptorControl(self.memory.0, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(control)
    }

    /// The DACL, or `None` for a NULL DACL, which lets everyone in.
    fn dacl(&self) -> Option<Dacl<'_>> {
        // SAFETY: a non-null `dacl` points at a valid ACL inside `memory`, which lives as long as
        // the borrow of `self` the result carries.
        unsafe { self.dacl.as_ref() }.map(|acl| Dacl { acl })
    }
}

/// A DACL borrowed from the `SecurityDescriptor` it lives in.
struct Dacl<'a> {
    acl: &'a ACL,
}

/// Where an allow or deny entry's SID starts: right after its header and mask.
const SID_OFFSET: usize = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
/// The smallest SID: revision, sub-authority count, and the six-byte authority, with no
/// sub-authorities.
const MIN_SID_LEN: usize = 8;

impl Dacl<'_> {
    /// The entries in order.
    fn entries(&self) -> io::Result<Vec<AccessEntry>> {
        (0..u32::from(self.acl.AceCount))
            .map(|index| self.entry(index))
            .collect()
    }

    /// Entry `index`, which must be below the ACL's count.
    fn entry(&self, index: u32) -> io::Result<AccessEntry> {
        let mut ace: *mut c_void = ptr::null_mut();
        // SAFETY: `acl` is a valid ACL, and `ace` is a valid out pointer. `GetAce` checks `index`
        // against the ACL's count itself.
        if unsafe { GetAce(self.acl, index, &mut ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: on success, `ace` points at entry `index` inside the ACL `self` borrows, and every
        // ACE starts with an `ACE_HEADER`.
        let header = unsafe { ace.cast::<ACE_HEADER>().as_ref() }.ok_or_else(unreadable_entry)?;
        if !matches!(header.AceType, ACCESS_ALLOWED_ACE_TYPE | ACCESS_DENIED_ACE_TYPE) {
            return Ok(AccessEntry {
                kind: header.AceType,
                flags: header.AceFlags,
                mask: 0,
                sid: None,
            });
        }
        // An entry is only as long as its header says: read no mask or SID past `AceSize`.
        let size = usize::from(header.AceSize);
        if size < SID_OFFSET + MIN_SID_LEN {
            return Err(unreadable_entry());
        }
        // SAFETY: allow and deny entries share `ACCESS_ALLOWED_ACE`'s layout, and `size` covers it.
        let mask = unsafe { ace.cast::<ACCESS_ALLOWED_ACE>().as_ref() }
            .ok_or_else(unreadable_entry)?
            .Mask;
        // Offset from the raw `ace`, so the pointer may reach the whole SID, past `SidStart`'s four
        // bytes.
        let sid: PSID = ace.cast::<u8>().wrapping_add(SID_OFFSET).cast();
        // SAFETY: `size` covers the SID's fixed eight bytes, all `IsValidSid` reads before it knows
        // the SID's length, and `GetLengthSid` runs only on a valid SID.
        if unsafe { IsValidSid(sid) } == 0 || unsafe { GetLengthSid(sid) } as usize > size - SID_OFFSET {
            return Err(unreadable_entry());
        }
        Ok(AccessEntry {
            kind: header.AceType,
            flags: header.AceFlags,
            mask,
            sid: Some(sid_string(sid)?),
        })
    }
}

/// A DACL entry that can't be read without reading past it: an allow or deny entry too short for
/// its SID, or a null entry from a `GetAce` that succeeded. No Windows file system stores either.
fn unreadable_entry() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "a DACL entry that can't be read safely")
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
