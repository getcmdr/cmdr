//! POSIX: `pread`, the mode bits, and the device and inode with both times to the nanosecond.

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::path::Path;

use cmdr_fs::file_identity::FileId;

/// Which version of a local file a read saw: its device and inode, its length,
/// and its modification and change times to the nanosecond. The change time
/// moves on any change at all, so a rewrite in place is refused at any length.
/// What a copy carries on from after an outage (`Volume::resume_read_stream`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalVersion {
    id: Option<FileId>,
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl LocalVersion {
    /// The version `stat`, the open's own, describes.
    pub(crate) fn of(_file: &File, stat: &Metadata, _path: &Path) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        Self {
            id: FileId::of(stat),
            len: stat.len(),
            mtime: (stat.mtime(), stat.mtime_nsec()),
            ctime: (stat.ctime(), stat.ctime_nsec()),
        }
    }
}

pub(crate) fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

pub(crate) fn owner_only(options: &mut OpenOptions) {
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
}

pub(crate) fn shown(os: &Path) -> String {
    os.to_string_lossy().into_owned()
}
