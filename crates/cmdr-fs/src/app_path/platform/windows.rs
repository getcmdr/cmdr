//! Windows: an `OsStr` is UTF-16 units (WTF-8 inside), so names escape through the unit
//! alphabet: an unpaired surrogate becomes a unit escape.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use super::super::escape::{self, Namer};
use super::super::{AppPath, AppPathBuf, NativeRefusal, NotAnAppPath, PathFlavor};

pub(crate) const HOST_FLAVOR: PathFlavor = PathFlavor::Windows;
pub(crate) const HOST_NAMER: Namer = Namer::Units;

pub(crate) fn os_name_to_app(os: &OsStr) -> Cow<'_, str> {
    match os.to_str() {
        Some(text) if !escape::has_escape(text) => Cow::Borrowed(text),
        _ => Cow::Owned(escape::escape_units(&os.encode_wide().collect::<Vec<u16>>())),
    }
}

pub(crate) fn app_to_os(text: &str) -> Cow<'_, OsStr> {
    if !escape::has_escape(text) {
        return Cow::Borrowed(OsStr::new(text));
    }
    Cow::Owned(OsString::from_wide(&bridge_units(text)))
}

impl AppPath {
    /// The path this OS's file APIs open: the verbatim `\\?\` form, which names exactly this
    /// file (no trailing-dot stripping, no device names, no `MAX_PATH`). The one way down to
    /// the OS.
    ///
    /// # Errors
    ///
    /// As [`AppPath::to_native_windows`]: a scheme path, a POSIX path (❗ Win32 would read
    /// `/x` as the CURRENT drive's root), a name holding `:` or another character Win32
    /// can't store, a UNC port, a byte escape, a bridge value, or past 32,767 units.
    pub fn to_native(&self) -> Result<PathBuf, NativeRefusal> {
        Ok(PathBuf::from(OsString::from_wide(&self.to_native_windows()?)))
    }

    /// The plain form (no `\\?\`), for the shell, a child process, and crates that can't
    /// take a verbatim path.
    ///
    /// # Errors
    ///
    /// As [`AppPath::to_native_windows_plain`]: everything `to_native` refuses, plus
    /// `NotPlainSafe` for a path whose plain form would name a different file.
    pub fn to_native_plain(&self) -> Result<PathBuf, NativeRefusal> {
        Ok(PathBuf::from(OsString::from_wide(&self.to_native_windows_plain()?)))
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
        Self::from_native_windows(&path.as_os_str().encode_wide().collect::<Vec<u16>>(), flavor)
    }
}

/// See the re-export in `mod.rs`: `GetLongPathNameW`.
pub fn expand_short_names(path: &Path) -> std::io::Result<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 512];
    loop {
        let capacity = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: `wide` is NUL-terminated and outlives the call; `buf` is writable for
        // `capacity` units, the size the call is told.
        let written = unsafe { GetLongPathNameW(wide.as_ptr(), buf.as_mut_ptr(), capacity) } as usize;
        if written == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if written < buf.len() {
            buf.truncate(written);
            return Ok(OsString::from_wide(&buf).into());
        }
        // Too small: `written` is the size it needs, its NUL included.
        buf.resize(written, 0);
    }
}

/// The UTF-16 units the transition bridge hands back for `text`: each unit escape as its
/// surrogate, everything else (a byte escape included) as its UTF-16.
fn bridge_units(text: &str) -> Vec<u16> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        match escape::unit_of(c) {
            Some(u) => out.push(u),
            None => out.extend_from_slice(c.encode_utf16(&mut [0u16; 2])),
        }
    }
    out
}
