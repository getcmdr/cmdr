//! Windows: the reader stats through each path's verbatim `\\?\` form, so a path past
//! `MAX_PATH` lists like any other, and its rows carry app paths (`C:/Users/test_user_1`,
//! plan D1). Always `read_dir`; no added date.
//!
//! Honest stubs, each owned by task 35 (windows-port plan, § "Windows build state"): no
//! allocation size (`FileStandardInfo.AllocationSize`), no mode (`0`, the "no permission
//! concept" value), and no owner (a SID's account name). Only `FILE_ATTRIBUTE_HIDDEN` is real.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs::{self, Metadata, ReadDir};
use std::io;
use std::os::windows::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use cmdr_fs::entry::FileEntry;

use super::super::{AddedAt, ListingDir, ListingTally, Pick, read_dir_entries};
use super::OsFields;
use crate::platform::local_windows::host_spelling;
use crate::platform::local_windows::paths::{app_of, native_of};

/// `FILE_ATTRIBUTE_HIDDEN`, what Explorer's "Hidden" checkbox sets.
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

/// A native path either way it's spelled (`C:\x`, the app path's `C:/x`, or `\\?\C:\x`) to its
/// app path and the verbatim form the OS opens.
pub(crate) fn located(path: &Path) -> io::Result<super::Located<'_>> {
    let app = app_of(path)?;
    let os = native_of(&app)?;
    Ok((Cow::Owned(os), Some(app)))
}

pub(crate) fn text_child(_dir: &Path, _name: &str) -> Option<PathBuf> {
    None
}

/// `\\?\C:\x` (or `C:\x`) as the app path `C:/x` the listing cache keys on.
pub(crate) fn app_spelling_of(os: &Path) -> Option<PathBuf> {
    app_of(os).ok().map(|app| host_spelling(&app))
}

/// Asks the folder for the entry's own name (`FindFirstFileExW`, which reads the directory
/// entry, not the file), so a case-only rename's old spelling reads as gone. A path with no last
/// name (a drive root) is trivially itself.
///
/// An 8.3 short name answers `false` too: its entry's name is the long one. That's the
/// conservative side for a watcher (the row isn't added under the short spelling), and
/// `ReadDirectoryChangesW` reports the name an operation used, which is the long one for every
/// program that doesn't type a short name.
pub(crate) fn named_exactly(os: &Path) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, WIN32_FIND_DATAW,
    };

    let Some(name) = os.file_name() else {
        return Ok(true);
    };
    let wide: Vec<u16> = os.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `WIN32_FIND_DATAW` is plain old data (integers and fixed arrays), so all-zero is
    // a valid value for the call to overwrite.
    let mut found: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    // SAFETY: `wide` is NUL-terminated, `found` is the `WIN32_FIND_DATAW` that
    // `FindExInfoBasic` fills, and the filter and flags are the documented "none" values.
    let handle = unsafe {
        FindFirstFileExW(
            wide.as_ptr(),
            FindExInfoBasic,
            (&raw mut found).cast(),
            FindExSearchNameMatch,
            std::ptr::null(),
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is the live search handle `FindFirstFileExW` just returned.
    unsafe { FindClose(handle) };
    let len = found
        .cFileName
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(found.cFileName.len());
    Ok(found.cFileName[..len].iter().copied().eq(name.encode_wide()))
}

pub(crate) fn lstat(path: &Path) -> io::Result<Metadata> {
    fs::symlink_metadata(path)
}

pub(crate) fn stat(path: &Path) -> io::Result<Metadata> {
    fs::metadata(path)
}

pub(crate) fn read_dir(path: &Path) -> io::Result<ReadDir> {
    fs::read_dir(path)
}

pub(crate) fn mode_of(_metadata: &Metadata) -> u32 {
    0
}

pub(crate) fn os_fields(metadata: &Metadata) -> OsFields {
    OsFields {
        flag_hidden: metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0,
        ..OsFields::default()
    }
}

pub(crate) fn read_entries(
    dir: &ListingDir<'_>,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
    pick: Pick,
) -> io::Result<Vec<FileEntry>> {
    let _ = pick;
    read_dir_entries(dir, dot_hidden, tally, AddedAt::Skip)
}

/// NTFS and ReFS record a birth time for every entry, so a re-stat already equals its row.
pub(crate) fn finish_single_entry(_path: &Path, entry: FileEntry) -> FileEntry {
    entry
}

pub(crate) fn added_is_supported(_dir: &Path) -> bool {
    false
}

#[cfg(any(test, feature = "testing"))]
pub(crate) fn added_at_by_path(_path: &Path) -> Option<u64> {
    None
}
