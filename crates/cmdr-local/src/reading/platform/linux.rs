//! Linux: always `read_dir`, no added date, no hidden flag, and a birth time kept only where
//! the folder's listing claims one (`statx` reports each entry's own filesystem).

use std::collections::HashSet;
use std::fs::Metadata;
use std::io;
use std::path::Path;

use cmdr_fs::entry::FileEntry;

use super::super::{
    AddedAt, ListingDir, ListingTally, Pick, read_dir_entries, supported_dates_of, withhold_unclaimed_dates,
};
use super::OsFields;

pub(crate) use super::unix::{app_spelling_of, located, lstat, mode_of, named_exactly, read_dir, stat, text_child};

pub(crate) fn os_fields(metadata: &Metadata) -> OsFields {
    super::unix::posix_fields(metadata, false)
}

/// Each entry keeps only the dates `path`'s listing claims (`withhold_unclaimed_dates`): one
/// more `stat`, of `path`.
pub(crate) fn read_entries(
    dir: &ListingDir<'_>,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
    pick: Pick,
) -> io::Result<Vec<FileEntry>> {
    let _ = pick;
    let entries = read_dir_entries(dir, dot_hidden, tally, AddedAt::Skip)?;
    let dates = supported_dates_of(dir.os());
    Ok(entries
        .into_iter()
        .map(|entry| withhold_unclaimed_dates(entry, dates))
        .collect())
}

/// Keeps the birth time only where the parent's listing claims one, as `read_entries` does,
/// so a re-stat equals its listing entry. The parent's `stat` runs only for an entry that has
/// a birth time to lose.
pub(crate) fn finish_single_entry(path: &Path, entry: FileEntry) -> FileEntry {
    if entry.created_at.is_some() {
        withhold_unclaimed_dates(entry, supported_dates_of(parent_dir(path)))
    } else {
        entry
    }
}

/// The folder whose listing holds `path` (`path` itself at `/`).
fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => Path::new("."),
        Some(parent) => parent,
        None => path,
    }
}

pub(crate) fn added_is_supported(_dir: &Path) -> bool {
    false
}

#[cfg(any(test, feature = "testing"))]
pub(crate) fn added_at_by_path(_path: &Path) -> Option<u64> {
    None
}
