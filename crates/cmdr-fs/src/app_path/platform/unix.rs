//! macOS and Linux: an `OsStr` is raw bytes, so names escape through the byte alphabet.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use super::super::escape::{self, Namer};
use super::super::{AppPath, AppPathBuf, NativeRefusal, NotAnAppPath, PathFlavor};

pub(crate) const HOST_FLAVOR: PathFlavor = PathFlavor::Posix;
pub(crate) const HOST_NAMER: Namer = Namer::Bytes;

pub(crate) fn os_name_to_app(os: &OsStr) -> Cow<'_, str> {
    escape::escape_bytes(os.as_bytes())
}

pub(crate) fn app_to_os(text: &str) -> Cow<'_, OsStr> {
    if !escape::has_escape(text) {
        return Cow::Borrowed(OsStr::new(text));
    }
    Cow::Owned(OsString::from_vec(bridge_bytes(text)))
}

impl AppPath {
    /// The path this OS's file APIs open: the POSIX bytes, each byte escape decoded. The one
    /// way down to the OS.
    ///
    /// # Errors
    ///
    /// As [`AppPath::to_native_posix`]: a scheme path, a drive or UNC path, a NUL, a unit
    /// escape, a bridge value.
    pub fn to_native(&self) -> Result<PathBuf, NativeRefusal> {
        Ok(PathBuf::from(OsStr::from_bytes(&self.to_native_posix()?)))
    }

    /// [`AppPath::to_native`]: macOS and Linux have no second, plain form.
    ///
    /// # Errors
    ///
    /// As [`AppPath::to_native`].
    pub fn to_native_plain(&self) -> Result<PathBuf, NativeRefusal> {
        self.to_native()
    }
}

impl AppPathBuf {
    /// The app path for a path this OS reported. Lossless: a name that isn't valid Unicode
    /// text is escaped, ❌ never spelled lossily. The one way up from the OS.
    ///
    /// # Errors
    ///
    /// As [`AppPathBuf::from_native_as`] with this host's flavor.
    pub fn from_native(path: &Path) -> Result<AppPathBuf, NotAnAppPath> {
        Self::from_native_as(path, PathFlavor::HOST)
    }

    /// [`AppPathBuf::from_native`] read with `flavor`'s grammar, for a test of the other one.
    ///
    /// # Errors
    ///
    /// As [`AppPathBuf::from_native_posix`] and [`AppPathBuf::from_native_windows`].
    pub fn from_native_as(path: &Path, flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath> {
        Self::from_native_posix(path.as_os_str().as_bytes(), flavor)
    }
}

/// No short names on a POSIX filesystem: `path` as is.
pub fn expand_short_names(path: &Path) -> std::io::Result<PathBuf> {
    Ok(path.to_path_buf())
}

/// The bytes the transition bridge hands back for `text`: each byte escape as its byte,
/// everything else (a unit escape included, which no byte can spell) as its UTF-8.
fn bridge_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        match escape::byte_of(c) {
            Some(b) => out.push(b),
            None => out.extend_from_slice(c.encode_utf8(&mut [0u8; 4]).as_bytes()),
        }
    }
    out
}
