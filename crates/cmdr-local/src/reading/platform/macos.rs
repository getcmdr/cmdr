//! macOS: `getattrlistbulk` on APFS and HFS+ (`reading/bulk.rs`), `read_dir` elsewhere, the
//! added date wherever the folder is bulk-eligible, and `UF_HIDDEN`.

use std::collections::HashSet;
use std::fs::Metadata;
use std::io;
use std::path::Path;

use cmdr_fs::entry::FileEntry;

use super::super::{ListingDir, ListingTally, Pick, bulk};
use super::OsFields;

pub(crate) use super::unix::{app_spelling_of, located, lstat, mode_of, named_exactly, read_dir, stat, text_child};

/// Whether a POSIX entry's `st_flags` carries `UF_HIDDEN`, what `chflags hidden` sets. Reads
/// a field the `lstat` that produced `metadata` already fetched, so it costs no syscall.
fn has_uf_hidden(metadata: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt as _;
    metadata.st_flags() & libc::UF_HIDDEN != 0
}

pub(crate) fn os_fields(metadata: &Metadata) -> OsFields {
    super::unix::posix_fields(metadata, has_uf_hidden(metadata))
}

/// macOS picks the lister (`bulk::read_entries`).
pub(crate) fn read_entries(
    dir: &ListingDir<'_>,
    dot_hidden: &HashSet<String>,
    tally: Option<&ListingTally>,
    pick: Pick,
) -> io::Result<Vec<FileEntry>> {
    bulk::read_entries(dir, dot_hidden, tally, pick)
}

pub(crate) fn finish_single_entry(path: &Path, entry: FileEntry) -> FileEntry {
    FileEntry {
        added_at: bulk::parent_is_bulk_eligible(path)
            .then(|| bulk::added_at_by_path(path))
            .flatten(),
        ..entry
    }
}

pub(crate) fn added_is_supported(dir: &Path) -> bool {
    bulk::dir_is_bulk_eligible(dir)
}

#[cfg(any(test, feature = "testing"))]
pub(crate) fn added_at_by_path(path: &Path) -> Option<u64> {
    bulk::added_at_by_path(path)
}
