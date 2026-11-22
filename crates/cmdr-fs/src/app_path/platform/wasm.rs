//! The browser build: no OS, so no native crossing at all. Its demo tree is POSIX, and an
//! `OsStr` there only ever holds the UTF-8 text a `str` made it from.

use std::borrow::Cow;
use std::ffi::OsStr;

use super::super::PathFlavor;
use super::super::escape::{self, Namer};

pub(crate) const HOST_FLAVOR: PathFlavor = PathFlavor::Posix;
pub(crate) const HOST_NAMER: Namer = Namer::Bytes;

pub(crate) fn os_name_to_app(os: &OsStr) -> Cow<'_, str> {
    escape::escape_bytes(os.as_encoded_bytes())
}

pub(crate) fn app_to_os(text: &str) -> Cow<'_, OsStr> {
    // Nothing here reads a disk, so an escape has no byte to stand for: the text stays.
    Cow::Borrowed(OsStr::new(text))
}

impl super::super::AppPath {
    /// No path has a native spelling in the browser: `None`, always.
    pub fn native_if_any(&self) -> Option<std::path::PathBuf> {
        None
    }
}
