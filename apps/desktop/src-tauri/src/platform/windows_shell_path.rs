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

#[cfg(test)]
mod tests {
    use super::*;
    use cmdr_fs::app_path::{AppPathBuf, NotPlainSafe, PathFlavor};

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
