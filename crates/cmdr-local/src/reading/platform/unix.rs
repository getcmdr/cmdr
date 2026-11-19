//! What macOS and Linux read the same way: paths go to the OS as they are, and an entry's
//! mode, owner, and allocation come off its `lstat`.

use std::borrow::Cow;
use std::fs::{self, Metadata, ReadDir};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

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

/// The OS path is the path: its rows keep the OS path's text, which on macOS and Linux is the
/// app path's own bytes.
pub(crate) fn located(path: &Path) -> io::Result<super::Located<'_>> {
    Ok((Cow::Borrowed(path), None))
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
