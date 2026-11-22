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

/// The find-data lister; a refresh's re-read by handle (`refresh_entries`); or the `lstat`
/// one, when a test or the benchmark forces it.
pub(crate) fn read_entries(
    dir: &ListingDir<'_>,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
    pick: Pick,
) -> io::Result<Vec<FileEntry>> {
    match pick {
        Pick::ByFilesystem => find_data_entries(dir, dot_hidden, tally),
        Pick::Refresh => refresh_entries(dir, dot_hidden, tally),
        #[cfg(any(test, feature = "testing"))]
        Pick::Forced(super::super::Lister::ReadDir) => read_dir_entries(dir, dot_hidden, tally, AddedAt::Skip),
    }
}

/// Every entry off the directory's own records: `read_dir` is `FindFirstFileExW` with basic
/// info, whose find data holds each entry's attributes, reparse tag, size, and three dates, the
/// fields an `lstat` here would read, so `DirEntry::metadata` costs no call. A listing of 20,000
/// files on the hosted runner's NTFS took a third of the time an `lstat` per entry took
/// (`docs/notes/windows-port/windows-lister-2026-11.md`). ❗ A directory record lags a file
/// another program holds open for writing: its size and last-write time catch up when that
/// handle closes, as Explorer shows too, and no change notification fires meanwhile. A refresh
/// reads past it (`refresh_entries`).
fn find_data_entries(
    dir: &ListingDir<'_>,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
) -> io::Result<Vec<FileEntry>> {
    let dir_entries: Vec<_> = read_dir(dir.os())?.collect();
    let mut entries = Vec::with_capacity(dir_entries.len());
    for entry in dir_entries {
        let file_entry = find_data_row(dir, &entry?, dot_hidden);
        if let Some(tally) = tally {
            tally.record(&file_entry);
        }
        entries.push(file_entry);
    }
    Ok(entries)
}

/// One entry's row off its find data.
fn find_data_row(dir: &ListingDir<'_>, entry: &fs::DirEntry, dot_hidden: &HashSet<String>) -> FileEntry {
    let (entry_path, row) = dir.child(&entry.file_name());
    // `metadata` can't fail on Windows (it's the find data already read); a failure would
    // still list the entry, through its own `lstat`.
    match entry.metadata() {
        Ok(metadata) => super::super::entry_from_metadata(&entry_path, row, &metadata, dot_hidden),
        Err(_) => super::super::entry_from_lstat(&entry_path, row, dot_hidden, AddedAt::Skip).unwrap_or_else(|row| {
            let is_symlink = entry.file_type().is_ok_and(|ft| ft.is_symlink());
            super::super::minimal_row(row, is_symlink, dot_hidden)
        }),
    }
}

/// How many entries a refresh re-reads by handle, at most: every entry of a folder up to this
/// size, else this many of its files, the most recently written first. About 20 ms of stats on
/// the hosted runner's NTFS (an `lstat` there costs 17–21 µs,
/// `docs/notes/windows-port/windows-lister-2026-11.md`), so a refresh of any folder stays
/// within that of its listing.
const REFRESH_RESTAT_CAP: usize = 1_000;

/// A refresh's listing (#1387): what a listing gives, except that the entries whose find data
/// can lag (`find_data_entries`) are re-read by handle, so a file another program is writing
/// shows its live size and modified time.
///
/// - **A folder of up to [`REFRESH_RESTAT_CAP`] entries**: every entry by its own `lstat`
///   (`read_dir_entries`), the lister Windows used before the find data.
/// - **A bigger one**: its find data, then the [`REFRESH_RESTAT_CAP`] files with the newest
///   find-data modified time re-read. Bounded, so a refresh of a huge folder stays fast. A file
///   being written was last recorded when its writer opened it or first grew it, so it ranks
///   among the folder's newest unless more than that many files were written since; such a
///   file keeps its recorded size until its writer closes it or the watcher hears it.
///
/// Why not "re-read what changed since the last listing": the lagging record doesn't change,
/// and NTFS sends no change notification for it, so neither signal names the file.
fn refresh_entries(
    dir: &ListingDir<'_>,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
) -> io::Result<Vec<FileEntry>> {
    // Counting the names is one enumeration with no rows built: about 4% of a find-data
    // listing (10 ms of 246 for 20,000 files).
    if read_dir(dir.os())?.take(REFRESH_RESTAT_CAP + 1).count() <= REFRESH_RESTAT_CAP {
        return read_dir_entries(dir, dot_hidden, tally, AddedAt::Skip);
    }
    let dir_entries = read_dir(dir.os())?.collect::<io::Result<Vec<_>>>()?;
    let mut rows: Vec<FileEntry> = dir_entries
        .iter()
        .map(|entry| find_data_row(dir, entry, dot_hidden))
        .collect();
    for index in newest_files(&rows, REFRESH_RESTAT_CAP) {
        let (entry_path, row) = dir.child(&dir_entries[index].file_name());
        // A file gone or unreadable since its find data keeps that row.
        if let Ok(fresh) = super::super::entry_from_lstat(&entry_path, row, dot_hidden, AddedAt::Skip) {
            rows[index] = fresh;
        }
    }
    if let Some(tally) = tally {
        for row in &rows {
            tally.record(row);
        }
    }
    Ok(rows)
}

/// The indices of `rows`' plain files (no folder, no link) with the newest modified time, at
/// most `cap` of them, in no order. A folder's or a link's find data is the item itself, which
/// a writer doesn't hold open.
fn newest_files(rows: &[FileEntry], cap: usize) -> Vec<usize> {
    let mut files: Vec<usize> = (0..rows.len())
        .filter(|&i| !rows[i].is_directory && !rows[i].is_symlink)
        .collect();
    if files.len() > cap {
        files.select_nth_unstable_by_key(cap, |&i| std::cmp::Reverse(rows[i].modified_at));
        files.truncate(cap);
    }
    files
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

#[cfg(test)]
#[path = "windows_refresh_test.rs"]
mod refresh_test;
