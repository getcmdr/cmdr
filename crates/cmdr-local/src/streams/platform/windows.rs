//! Windows: `ReadFile` at an offset, no mode, the node and change time read off the open
//! handle, and errors named by the app path.

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;

use cmdr_fs::file_identity::FileId;
use windows_sys::Win32::Storage::FileSystem::{FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx};

/// Which version of a local file a read saw: its node (volume serial and file id), its
/// length, and its last-write, creation, and change times in 100 ns ticks, every one read
/// off the handle the read opened, so nothing swapped in at the name since can answer for it.
///
/// The change time (`FILE_BASIC_INFO.ChangeTime`, the NTFS and ReFS twin of POSIX's `ctime`)
/// moves on any write or metadata change, putting the last-write time back included, so a
/// rewrite in place is refused at any length, as on POSIX. FAT and exFAT keep none and
/// answer zero: there the last-write time is the whole check, all the filesystem offers. A
/// handle that won't answer reads `None`, which only matches another `None`, so the other
/// fields decide.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalVersion {
    id: Option<FileId>,
    len: u64,
    last_write: u64,
    created: u64,
    changed: Option<i64>,
}

impl LocalVersion {
    /// The version the open `file` and its own `stat` describe.
    pub(crate) fn of(file: &File, stat: &Metadata, _path: &Path) -> Self {
        use std::os::windows::fs::MetadataExt as _;
        Self {
            id: FileId::of_open(file),
            len: stat.len(),
            last_write: stat.last_write_time(),
            created: stat.creation_time(),
            changed: change_time(file),
        }
    }
}

/// The handle's `FILE_BASIC_INFO.ChangeTime`, or `None` when the call fails.
fn change_time(file: &File) -> Option<i64> {
    let mut info = FILE_BASIC_INFO::default();
    let size = u32::try_from(size_of::<FILE_BASIC_INFO>()).ok()?;
    // SAFETY: the handle is `file`'s, open for this whole call, and `info` is a
    // `FILE_BASIC_INFO` of exactly `size` bytes, the buffer `FileBasicInfo` fills.
    let ok = unsafe { GetFileInformationByHandleEx(file.as_raw_handle(), FileBasicInfo, (&raw mut info).cast(), size) };
    (ok != 0).then_some(info.ChangeTime)
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
