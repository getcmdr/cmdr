//! The local reader's per-OS half: how a path reaches the OS, which lister runs, and what an
//! entry's stat says beyond std's portable fields. The listing loop, the tally, and the
//! entry's shape stay in `reading.rs`, the same on every OS.

use std::collections::HashSet;
use std::fs::{Metadata, ReadDir};
use std::io;
use std::path::{Path, PathBuf};

use cmdr_fs::entry::FileEntry;

use super::{ListingTally, Pick};

#[cfg(unix)]
mod unix;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

/// What an entry's own stat says beyond std's portable fields, per OS.
// DEFAULT-OK: the zero value is "the OS says nothing more": no allocation figure, no mode,
// no owner, not hidden by a flag. Each arm fills what its stat carries.
#[derive(Default)]
pub(crate) struct OsFields {
    /// Bytes the file takes on disk (`st_blocks * 512` on POSIX), files only.
    pub(crate) physical_size: Option<u64>,
    /// POSIX mode bits, `0` where the OS has none.
    pub(crate) permissions: u32,
    /// The owning user's name, empty when unknown.
    pub(crate) owner: String,
    /// The owning group's name, empty when unknown.
    pub(crate) group: String,
    /// The OS's own hidden flag: `UF_HIDDEN` on macOS, `FILE_ATTRIBUTE_HIDDEN` on Windows.
    pub(crate) flag_hidden: bool,
}

const _: fn(&Path) -> io::Result<Metadata> = lstat;
const _: fn(&Path) -> io::Result<Metadata> = stat;
const _: fn(&Path) -> io::Result<ReadDir> = read_dir;
const _: fn(&Path, &str) -> PathBuf = child_path;
const _: fn(&Metadata) -> OsFields = os_fields;
const _: fn(&Metadata) -> u32 = mode_of;
/// `read_entries`' shape: the folder, its `/.hidden` names, the tally, and who picks the lister.
type ReadEntries = fn(&Path, &HashSet<String>, Option<&ListingTally>, Pick) -> io::Result<Vec<FileEntry>>;
const _: ReadEntries = read_entries;
const _: fn(&Path, FileEntry) -> FileEntry = finish_single_entry;
const _: fn(&Path) -> bool = added_is_supported;
#[cfg(any(test, feature = "testing"))]
const _: fn(&Path) -> Option<u64> = added_at_by_path;

/// The entry at `path` itself, a link not followed. `path` is the reader's path (an app path
/// on Windows, which this turns into the OS's own form).
pub(crate) use imp::lstat;

/// The entry at `path`, a link followed to its target.
pub(crate) use imp::stat;

/// The names in the folder at `path`, as the OS lists them.
pub(crate) use imp::read_dir;

/// The reader's path for the entry `name` inside the folder at `dir`, the spelling a
/// `FileEntry` carries.
pub(crate) use imp::child_path;

/// [`OsFields`] from an entry's stat. Resolves owner names, so call it once per entry.
pub(crate) use imp::os_fields;

/// The mode bits alone, for a log line.
pub(crate) use imp::mode_of;

/// Every entry of `path`, unsorted, through the lister this OS and `pick` choose.
pub(crate) use imp::read_entries;

/// `entry`, built by `get_single_entry` from its own stat, made equal to its row in its
/// folder's listing (the added date macOS fills, the birth time Linux withholds).
pub(crate) use imp::finish_single_entry;

/// Whether a listing of the folder `dir` carries the added date.
pub(crate) use imp::added_is_supported;

/// The added date of the entry at `path`, where the OS records one.
#[cfg(any(test, feature = "testing"))]
pub(crate) use imp::added_at_by_path;
