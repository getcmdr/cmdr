//! What a failed local rename names, shared by both local volumes.

use std::io;
use std::path::Path;

use cmdr_fs::volume::VolumeError;

/// Which of a rename's two paths an `io::Error` is talking about.
///
/// `ENOENT` is the SOURCE that isn't there; `EEXIST` and `ENOTEMPTY` are the
/// DESTINATION that already is. Handing both to the same path would name the
/// wrong file in "there's already something called that", which is the one
/// sentence in the rename flow the user acts on.
pub fn rename_volume_error(err: &io::Error, from: &Path, to: &Path) -> VolumeError {
    match err.kind() {
        io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty => VolumeError::from_io_at(err, to),
        _ => VolumeError::from_io_at(err, from),
    }
}
