//! Windows: the reader reads through each path's verbatim `\\?\` form, so a path past
//! `MAX_PATH` lists like any other, and its rows carry app paths (`C:/Users/test_user_1`,
//! plan D1). A listing's rows come off `read_dir`'s find data, with no call per entry
//! (`find_data_entries`); a single entry stats. No added date.
//!
//! Honest stubs (windows-port plan, § "Windows build state"): no allocation size
//! (`FileStandardInfo.AllocationSize`) and no mode (`0`, the "no permission concept" value).
//! Only `FILE_ATTRIBUTE_HIDDEN` is real. No owner either, by decision: a Windows owner is a SID
//! read from each entry's security descriptor (a handle per entry, the very call the find-data
//! lister saves), and nothing shows `FileEntry::owner` yet (`crates/cmdr-local/DETAILS.md` §
//! "The Windows sibling").

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

/// The find-data lister, unless a test or the benchmark forces the `lstat` one.
pub(crate) fn read_entries(
    dir: &ListingDir<'_>,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
    pick: Pick,
) -> io::Result<Vec<FileEntry>> {
    #[cfg(any(test, feature = "testing"))]
    if matches!(pick, Pick::Forced(super::super::Lister::ReadDir)) {
        return read_dir_entries(dir, dot_hidden, tally, AddedAt::Skip);
    }
    let _ = pick;
    find_data_entries(dir, dot_hidden, tally)
}

/// Every entry off the directory's own records: `read_dir` is `FindFirstFileExW` with basic
/// info, whose find data holds each entry's attributes, reparse tag, size, and three dates, the
/// fields an `lstat` here would read, so `DirEntry::metadata` costs no call. A listing of 20,000
/// files on the hosted runner's NTFS took a third of the time an `lstat` per entry took
/// (`docs/notes/windows-port/windows-lister-2026-11.md`). ❗ A directory record can lag a file
/// another program holds open for writing (its size and last-write time catch up when that
/// handle closes, as Explorer shows too); the watcher's re-stat of a changed entry
/// (`get_single_entry`) reads the live answer.
fn find_data_entries(
    dir: &ListingDir<'_>,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
) -> io::Result<Vec<FileEntry>> {
    let dir_entries: Vec<_> = read_dir(dir.os())?.collect();
    let mut entries = Vec::with_capacity(dir_entries.len());
    for entry in dir_entries {
        let entry = entry?;
        let (entry_path, row) = dir.child(&entry.file_name());
        // `metadata` can't fail on Windows (it's the find data already read); a failure would
        // still list the entry, through its own `lstat`.
        let file_entry = match entry.metadata() {
            Ok(metadata) => super::super::entry_from_metadata(&entry_path, row, &metadata, dot_hidden),
            Err(_) => {
                super::super::entry_from_lstat(&entry_path, row, dot_hidden, AddedAt::Skip).unwrap_or_else(|row| {
                    let is_symlink = entry.file_type().is_ok_and(|ft| ft.is_symlink());
                    super::super::minimal_row(row, is_symlink, dot_hidden)
                })
            }
        };
        if let Some(tally) = tally {
            tally.record(&file_entry);
        }
        entries.push(file_entry);
    }
    Ok(entries)
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
