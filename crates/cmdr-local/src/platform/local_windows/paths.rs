//! The one place the Windows local volume crosses between app paths (`C:/Users/test_user_1`,
//! `//server/share/x`; plan D1) and the paths Win32 opens (`\\?\C:\Users\test_user_1`).
//!
//! Down, every call into the OS goes through [`native_of`] (`AppPath::to_native`), so a path
//! past `MAX_PATH` (260 UTF-16 units) reaches the filesystem whole: the `\\?\` form skips
//! Win32's length limit and its own normalization (verified on Windows 10 22H2, NTFS, the
//! long-path cells in `local_windows_test.rs`, 2026-11-18), and a trailing dot or space, or a
//! device name (`dup.`, `con.txt`), names exactly that file. Up, a native path a caller hands
//! the crate's `&Path` entry points crosses through [`app_of`] (`from_native_unicode`).

use std::io;
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, AppPathBuf, NativeRefusal, NotAnAppPath};

/// The `\\?\` path Win32 opens for the app path `path`.
///
/// # Errors
///
/// `AppPath::to_native`'s typed refusal, as an `io::Error` whose kind says what it is: a name
/// Win32 can't store (`:`, which would write an alternate data stream INSIDE another file,
/// `*?"<>|`) or a path past its ceiling is `InvalidFilename` (`VolumeError::InvalidName`);
/// anything else (a POSIX `/x`, which Win32 would read as the CURRENT drive's root, a scheme
/// path, a bridge value) is `InvalidInput`. The refusal rides as the error's source.
pub(crate) fn native_of(path: &AppPath) -> io::Result<PathBuf> {
    path.to_native().map_err(refused)
}

/// The app path of a native path a caller hands in: `C:\x`, its app spelling `C:/x`, or the
/// verbatim `\\?\C:\x`.
///
/// # Errors
///
/// `InvalidInput` carrying the typed [`NotAnAppPath`]: a relative path, `C:foo`, a device
/// path, or a name that isn't Unicode text (an unpaired surrogate, until escapes go live).
pub(crate) fn app_of(path: &Path) -> io::Result<AppPathBuf> {
    AppPathBuf::from_native_unicode(path).map_err(not_an_app_path)
}

/// A typed refusal of an app path as an `io::Error` a `VolumeError` classifies.
pub(crate) fn not_an_app_path(why: NotAnAppPath) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why)
}

fn refused(why: NativeRefusal) -> io::Error {
    let kind = match why {
        NativeRefusal::ForbiddenCharacter | NativeRefusal::TooLong => io::ErrorKind::InvalidFilename,
        _ => io::ErrorKind::InvalidInput,
    };
    io::Error::new(kind, why)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmdr_fs::app_path::PathFlavor;

    fn app(text: &str) -> AppPathBuf {
        AppPathBuf::parse(text, PathFlavor::Windows).expect("an app path")
    }

    #[test]
    fn an_app_path_opens_through_its_long_path_form() {
        assert_eq!(
            native_of(&app("C:/Users/test_user_1")).unwrap(),
            PathBuf::from(r"\\?\C:\Users\test_user_1")
        );
        assert_eq!(native_of(&app("C:/")).unwrap(), PathBuf::from(r"\\?\C:\"));
        assert_eq!(
            native_of(&app("//testnas.local/share/x")).unwrap(),
            PathBuf::from(r"\\?\UNC\testnas.local\share\x")
        );
    }

    /// A trailing dot, a trailing space, and a device name reach the exact file verbatim.
    #[test]
    fn a_name_the_plain_form_would_misread_keeps_its_verbatim_spelling() {
        assert_eq!(native_of(&app("C:/x/dup.")).unwrap(), PathBuf::from(r"\\?\C:\x\dup."));
        assert_eq!(native_of(&app("C:/x/a ")).unwrap(), PathBuf::from(r"\\?\C:\x\a "));
        assert_eq!(
            native_of(&app("C:/x/con.txt")).unwrap(),
            PathBuf::from(r"\\?\C:\x\con.txt")
        );
    }

    /// ❗ `:` never reaches the OS: `C:/x/a:b` would write stream `b` INSIDE file `a`.
    #[test]
    fn a_name_win32_cant_store_is_an_invalid_name() {
        let err = native_of(&app("C:/x/a:b")).expect_err("refused");
        assert_eq!(err.kind(), io::ErrorKind::InvalidFilename);
    }

    #[test]
    fn a_path_that_names_no_drive_is_refused() {
        let posix = AppPathBuf::parse("/Users/x", PathFlavor::Posix).expect("parses");
        assert_eq!(
            native_of(&posix).expect_err("refused").kind(),
            io::ErrorKind::InvalidInput
        );
        for bad in ["relative/x", r"C:foo"] {
            let err = app_of(Path::new(bad)).expect_err(bad);
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad}");
        }
    }

    /// A native path a caller hands in reads the same whichever separator spells it.
    #[test]
    fn a_native_path_crosses_up_either_way_it_is_spelled() {
        for spelled in [r"C:\Users\x", "C:/Users/x", r"\\?\C:\Users\x"] {
            assert_eq!(app_of(Path::new(spelled)).unwrap().as_str(), "C:/Users/x", "{spelled}");
        }
    }
}
