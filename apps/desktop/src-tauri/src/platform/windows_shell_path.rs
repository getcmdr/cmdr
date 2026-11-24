//! The path text the Windows Shell takes for an app path: the plain spelling (no `\\?\`), as
//! NUL-terminated UTF-16. `ShellExecuteExW`, `SHParseDisplayName`, and the app the Shell launches
//! all read a path the way Win32 reads a plain one, so a path whose plain form would name a
//! different file (a trailing dot or space, a device name like `CON`) or that runs past 259 units
//! is refused, typed, ❌ never sent verbatim or with `\\?\` stripped by hand.
//!
//! Pure text, so its tests run on every OS (`PathFlavor::Windows` parses on any host).

use cmdr_fs::app_path::{AppPath, NativeRefusal};

/// `path` as the Shell's NUL-terminated wide string.
///
/// # Errors
///
/// As [`AppPath::to_native_windows_plain`]: [`NativeRefusal::NotPlainSafe`] for a long path, a
/// name ending in a dot or space, or a device name; [`NativeRefusal::NotLocal`] for a scheme
/// path; and the rest for text no Windows file has.
pub(crate) fn shell_path(path: &AppPath) -> Result<Vec<u16>, NativeRefusal> {
    let mut units = path.to_native_windows_plain()?;
    units.push(0);
    Ok(units)
}

/// The Shell's paths for a launch: the item's own, and its folder's, the working folder a
/// program started from it gets (as File Explorer gives it). The folder is `None` at a root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellLaunchPaths {
    /// The item, NUL-terminated.
    pub(crate) item: Vec<u16>,
    /// Its folder, NUL-terminated, or `None` for a drive or share root.
    pub(crate) folder: Option<Vec<u16>>,
}

/// [`shell_path`] for `path` and its folder.
///
/// # Errors
///
/// As [`shell_path`] for `path` itself. A folder of a path with a plain form has one too, since
/// every name in it already passed.
pub(crate) fn shell_launch_paths(path: &AppPath) -> Result<ShellLaunchPaths, NativeRefusal> {
    use cmdr_fs::app_path::PathArithmetic;

    let item = shell_path(path)?;
    let folder = path.parent().map(shell_path).transpose()?;
    Ok(ShellLaunchPaths { item, folder })
}

/// The Win32 error code an `HRESULT` wraps (`HRESULT_FROM_WIN32`, facility 7), or `None` for
/// any other `HRESULT`: `SHParseDisplayName` reports a missing file as `0x80070002`, which reads
/// as `ERROR_FILE_NOT_FOUND`, the same code `ShellExecuteExW` sets.
pub(crate) fn win32_code(hresult: i32) -> Option<u32> {
    let bits = hresult.cast_unsigned();
    (bits & 0xFFFF_0000 == 0x8007_0000).then_some(bits & 0xFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmdr_fs::app_path::{AppPathBuf, NotPlainSafe, PathFlavor};

    fn text(units: &[u16]) -> String {
        String::from_utf16(&units[..units.len() - 1]).expect("plain text")
    }

    #[test]
    fn a_launch_gets_the_items_folder_as_its_working_folder() {
        let path = AppPathBuf::parse("C:/Tools/setup.bat", PathFlavor::Windows).expect("a Windows app path");
        let paths = shell_launch_paths(&path).expect("plain");
        assert_eq!(text(&paths.item), r"C:\Tools\setup.bat");
        assert_eq!(paths.folder.as_deref().map(text), Some(r"C:\Tools".to_string()));
    }

    #[test]
    fn a_file_at_a_drive_root_gets_the_root_and_a_root_gets_none() {
        let file = AppPathBuf::parse("D:/x.exe", PathFlavor::Windows).expect("a Windows app path");
        assert_eq!(
            shell_launch_paths(&file).expect("plain").folder.as_deref().map(text),
            Some(r"D:\".to_string())
        );
        let root = AppPathBuf::parse("D:/", PathFlavor::Windows).expect("a Windows app path");
        assert_eq!(shell_launch_paths(&root).expect("plain").folder, None);
    }

    #[test]
    fn a_win32_hresult_unwraps_and_any_other_does_not() {
        assert_eq!(win32_code(0x8007_0002_u32.cast_signed()), Some(2));
        assert_eq!(win32_code(0x8007_04C7_u32.cast_signed()), Some(1223));
        assert_eq!(win32_code(0x8000_FFFF_u32.cast_signed()), None);
        assert_eq!(win32_code(0), None);
    }

    fn shell(text: &str) -> Result<String, NativeRefusal> {
        let path = AppPathBuf::parse(text, PathFlavor::Windows).expect("a Windows app path");
        let units = shell_path(&path)?;
        assert_eq!(units.last(), Some(&0), "NUL-terminated");
        Ok(String::from_utf16(&units[..units.len() - 1]).expect("plain text"))
    }

    #[test]
    fn a_drive_path_goes_plain_with_backslashes() {
        assert_eq!(
            shell("C:/Users/test_user_1/report.pdf"),
            Ok(r"C:\Users\test_user_1\report.pdf".into())
        );
    }

    #[test]
    fn a_drive_root_keeps_its_backslash() {
        assert_eq!(shell("D:/"), Ok(r"D:\".into()));
    }

    #[test]
    fn a_unc_path_goes_plain_without_the_verbatim_prefix() {
        assert_eq!(
            shell("//testnas.local/share/a b/x.txt"),
            Ok(r"\\testnas.local\share\a b\x.txt".into())
        );
    }

    #[test]
    fn a_comma_or_a_space_in_a_name_stays_as_is() {
        // The reason for the Shell call over `explorer.exe /select,…`: a comma splits its argument.
        assert_eq!(shell("C:/a, b/c,d.txt"), Ok(r"C:\a, b\c,d.txt".into()));
    }

    #[test]
    fn a_path_past_259_units_is_refused() {
        let long = format!("C:/{}", "x".repeat(300));
        assert_eq!(shell(&long), Err(NativeRefusal::NotPlainSafe(NotPlainSafe::TooLong)));
    }

    #[test]
    fn a_name_the_plain_form_would_change_is_refused() {
        assert_eq!(
            shell("C:/temp/dup."),
            Err(NativeRefusal::NotPlainSafe(NotPlainSafe::TrailingDotOrSpace))
        );
        assert_eq!(
            shell("C:/temp/con.txt"),
            Err(NativeRefusal::NotPlainSafe(NotPlainSafe::ReservedName))
        );
    }

    #[test]
    fn a_scheme_path_has_no_shell_path() {
        let path = AppPathBuf::parse("sftp://test_user_1@testnas.local:22/x.txt", PathFlavor::Windows)
            .expect("a scheme path parses");
        assert_eq!(shell_path(&path), Err(NativeRefusal::NotLocal));
    }
}
