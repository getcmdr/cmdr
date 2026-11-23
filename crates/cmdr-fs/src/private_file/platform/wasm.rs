//! The browser build has no local disk and no users: a plain exclusive create, which `std` refuses
//! there anyway.

use std::fs::File;
use std::io;
use std::path::Path;

/// A plain exclusive create.
pub(crate) fn create_new(path: &Path) -> io::Result<File> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// A browser has no users to keep a file from.
pub(crate) fn is_private_to_current_user(_path: &Path) -> io::Result<bool> {
    Err(io::ErrorKind::Unsupported.into())
}
