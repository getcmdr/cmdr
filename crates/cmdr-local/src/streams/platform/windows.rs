//! Windows: `ReadFile` at an offset, no mode, the node read through a handle, and errors named
//! by the app path.

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::path::Path;

use cmdr_fs::file_identity::{FileId, Follow};

/// Which version of a local file a read saw: its node (volume serial and file id, read by
/// path right after the open, since stable std's `Metadata` carries none), its length, and
/// its last-write and creation times in 100 ns ticks. Windows' change time isn't on stable
/// std, so a rewrite in place that keeps the length AND sets the last-write time back is the
/// one change this misses (the POSIX twin's change time catches it).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalVersion {
    id: Option<FileId>,
    len: u64,
    last_write: u64,
    created: u64,
}

impl LocalVersion {
    /// The version `stat`, the open's own, describes, plus the node at `path`.
    pub(crate) fn of(_file: &File, stat: &Metadata, path: &Path) -> Self {
        use std::os::windows::fs::MetadataExt as _;
        Self {
            id: FileId::at(path, Follow::Links),
            len: stat.len(),
            last_write: stat.last_write_time(),
            created: stat.creation_time(),
        }
    }
}

/// `seek_read` moves the handle's cursor too; every caller here reads positionally only.
pub(crate) fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

pub(crate) fn owner_only(_options: &mut OpenOptions) {}

pub(crate) fn shown(os: &Path) -> String {
    crate::platform::local_windows::paths::app_of(os).map_or_else(
        |_| os.to_string_lossy().into_owned(),
        cmdr_fs::app_path::AppPathBuf::into_string,
    )
}
