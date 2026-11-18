//! Windows: the reader takes and hands back app paths (`C:/Users/test_user_1`, plan D1) and
//! reaches the OS through their `\\?\` form, so a path past `MAX_PATH` lists like any other.
//! Always `read_dir`; no added date.
//!
//! Honest stubs, each owned by task 35 (windows-port plan, § "Windows build state"): no
//! allocation size (`FileStandardInfo.AllocationSize`), no mode (`0`, the "no permission
//! concept" value), and no owner (a SID's account name). Only `FILE_ATTRIBUTE_HIDDEN` is real.

use std::collections::HashSet;
use std::fs::{self, Metadata, ReadDir};
use std::io;
use std::os::windows::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use cmdr_fs::entry::FileEntry;

use super::super::{AddedAt, ListingTally, Pick, read_dir_entries};
use super::OsFields;
use crate::platform::local_windows::paths::{child_app_path, native};

/// `FILE_ATTRIBUTE_HIDDEN`, what Explorer's "Hidden" checkbox sets.
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

pub(crate) fn lstat(path: &Path) -> io::Result<Metadata> {
    fs::symlink_metadata(native(path)?)
}

pub(crate) fn stat(path: &Path) -> io::Result<Metadata> {
    fs::metadata(native(path)?)
}

pub(crate) fn read_dir(path: &Path) -> io::Result<ReadDir> {
    fs::read_dir(native(path)?)
}

pub(crate) fn child_path(dir: &Path, name: &str) -> PathBuf {
    child_app_path(dir, name)
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
    path: &Path,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
    pick: Pick,
) -> io::Result<Vec<FileEntry>> {
    let _ = pick;
    read_dir_entries(path, dot_hidden, tally, AddedAt::Skip)
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
