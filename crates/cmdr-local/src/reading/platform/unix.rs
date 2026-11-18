//! What macOS and Linux read the same way: paths go to the OS as they are, and an entry's
//! mode, owner, and allocation come off its `lstat`.

use std::ffi::OsStr;
use std::fs::{self, Metadata, ReadDir};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use cmdr_fs::entry::{get_group_name, get_owner_name};

use super::OsFields;

pub(crate) fn lstat(path: &Path) -> io::Result<Metadata> {
    fs::symlink_metadata(path)
}

pub(crate) fn stat(path: &Path) -> io::Result<Metadata> {
    fs::metadata(path)
}

pub(crate) fn read_dir(path: &Path) -> io::Result<ReadDir> {
    fs::read_dir(path)
}

/// The child at the OS's own spelling of `name`: a name that isn't UTF-8 (Linux allows one)
/// is stat'ed through its raw bytes, never its lossy listing name.
pub(crate) fn child_path(dir: &Path, name: &OsStr) -> PathBuf {
    dir.join(name)
}

pub(crate) fn mode_of(metadata: &Metadata) -> u32 {
    metadata.permissions().mode()
}

/// [`OsFields`] off a POSIX stat, with the arm's own hidden flag.
pub(crate) fn posix_fields(metadata: &Metadata, flag_hidden: bool) -> OsFields {
    OsFields {
        physical_size: metadata.is_file().then(|| metadata.blocks() * 512),
        permissions: mode_of(metadata),
        owner: get_owner_name(metadata.uid()),
        group: get_group_name(metadata.gid()),
        flag_hidden,
    }
}
