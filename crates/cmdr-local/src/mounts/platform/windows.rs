//! Windows: the mount manager's own table. Every drive letter (`GetLogicalDriveStringsW`) and
//! every folder a volume is mounted in (the `FindFirstVolumeW` sweep and each volume's
//! `GetVolumePathNamesForVolumeNameW`), and, for one path, `GetVolumeNameForVolumeMountPointW`,
//! which answers only for a drive root or a volume mount point.
//!
//! - **Spelling**: [`list`] spells each mount the way `std::fs::canonicalize` does, verbatim
//!   (`\\?\C:\`, `\\?\C:\Mnt\Data`), because its caller resolves a folder that way and asks
//!   which listed mounts sit under it with `Path::strip_prefix`, which compares components
//!   exactly. A folder mount takes its on-disk case from the folder itself (the reparse point,
//!   opened without following it), since the mount manager keeps the case it was given.
//!   [`is_mount_point`] takes any spelling (`C:/`, `C:\`, `\\?\C:\`).
//! - **What isn't a mount**: a junction, even one to a drive root, and a plain folder or file.
//! - **A network path** is never a local mount: a share can't hold a volume mount point, and
//!   asking the server could block on a dead one. A bare share root (`\\server\share\`) is
//!   "can't say", which the write engine's guards read as a boundary.
//!
//! What Windows 10 22H2 answers (`ssh mami`, PowerShell P/Invoke, 2026-11-21):
//! `GetVolumeNameForVolumeMountPointW` gives the GUID path for `C:\`, `\\?\C:\`, `D:\`, and an
//! empty optical drive's `E:\`; `ERROR_FILE_NOT_FOUND` (2) for an unused letter and a missing
//! folder; `ERROR_NOT_A_REPARSE_POINT` (4390) for a plain folder; `ERROR_INVALID_NAME` (123)
//! for a file; `ERROR_INVALID_PARAMETER` (87) for a junction (to `C:\`, `D:\`, or a folder) and
//! a UNC root. `GetFinalPathNameByHandleW` on a junction opened with
//! `FILE_FLAG_OPEN_REPARSE_POINT` names the junction itself in its on-disk case
//! (`…\JFolder` for `…\jfolder`); followed, it names the target (`\\?\D:\`).

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    ERROR_BAD_PATHNAME, ERROR_FILE_NOT_FOUND, ERROR_INVALID_NAME, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA,
    ERROR_NO_MORE_FILES, ERROR_NOT_A_REPARSE_POINT, ERROR_PATH_NOT_FOUND, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FindFirstVolumeW, FindNextVolumeW, FindVolumeClose,
    GetDriveTypeW, GetFinalPathNameByHandleW, GetLogicalDriveStringsW, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNamesForVolumeNameW, VOLUME_NAME_DOS,
};

/// `GetDriveTypeW`'s answer for a network drive (`WindowsProgramming`'s constant, a feature
/// this crate doesn't otherwise need).
const DRIVE_REMOTE: u32 = 4;

/// A volume GUID path (`\\?\Volume{…}\`) is 49 UTF-16 units plus its NUL.
const GUID_PATH_LEN: usize = 50;

pub(crate) fn list() -> Option<Vec<PathBuf>> {
    let mut mounts: Vec<PathBuf> = drive_letters()?
        .into_iter()
        .map(|letter| PathBuf::from(format!(r"\\?\{letter}:\")))
        .collect();
    for path in mount_paths()? {
        if drive_root_letter(&path).is_none() {
            mounts.push(folder_mount_as_canonical(&path));
        }
    }
    Some(mounts)
}

pub(crate) fn is_mount_point(path: &Path) -> Option<bool> {
    let native = Native::of(path);
    if native.is_unc {
        // A share holds no local mount; its own root is the server's to answer.
        return if native.is_share_root() { None } else { Some(false) };
    }
    if let Some(letter) = drive_root_letter(&native.plain) {
        return Some(drive_letters()?.contains(&letter));
    }
    let root = native.plain.get(..3).unwrap_or_default();
    // SAFETY: `wide` is NUL-terminated.
    if unsafe { GetDriveTypeW(wide(root).as_ptr()) } == DRIVE_REMOTE {
        return Some(false);
    }
    let asked = if native.plain.len() < 248 {
        native.plain.clone()
    } else {
        format!(r"\\?\{}", native.plain)
    };
    let mut name = [0u16; GUID_PATH_LEN];
    // SAFETY: the asked path is NUL-terminated and `name` holds `GUID_PATH_LEN` units.
    let ok =
        unsafe { GetVolumeNameForVolumeMountPointW(wide(&asked).as_ptr(), name.as_mut_ptr(), GUID_PATH_LEN as u32) };
    if ok != 0 {
        return Some(true);
    }
    let code = std::io::Error::last_os_error().raw_os_error();
    let not_a_mount = code.and_then(|code| u32::try_from(code).ok()).is_some_and(|code| {
        matches!(
            code,
            ERROR_NOT_A_REPARSE_POINT
                | ERROR_INVALID_PARAMETER
                | ERROR_INVALID_NAME
                | ERROR_FILE_NOT_FOUND
                | ERROR_PATH_NOT_FOUND
                | ERROR_BAD_PATHNAME
        )
    });
    // Anything else (access denied, a drive that isn't ready) can't say.
    not_a_mount.then_some(false)
}

/// The drive letters in use (`GetLogicalDriveStringsW`), uppercase, mapped and `subst` ones
/// included; `None` when Windows wouldn't answer.
fn drive_letters() -> Option<Vec<char>> {
    let mut buffer = vec![0u16; 256];
    for _ in 0..2 {
        let len = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: `buffer` holds `len` units, and the call writes at most that many; a larger
        // answer is the length it needs, with nothing written.
        let written = unsafe { GetLogicalDriveStringsW(len, buffer.as_mut_ptr()) } as usize;
        if written == 0 {
            return None;
        }
        if written <= buffer.len() {
            return Some(
                String::from_utf16_lossy(&buffer[..written])
                    .split('\0')
                    .filter_map(|root| root.chars().next())
                    .filter(char::is_ascii_alphabetic)
                    .map(|letter| letter.to_ascii_uppercase())
                    .collect(),
            );
        }
        buffer = vec![0u16; written + 1];
    }
    None
}

/// Every place a volume the mount manager knows is mounted (`D:\`, `C:\Mnt\Data\`), from the
/// GUID sweep; `None` when the sweep wouldn't start or stopped short. A mapped or `subst`
/// drive is no volume here: [`drive_letters`] has those.
///
/// ❗ A partial sweep is `None`, ❌ never the volumes read so far: a missed folder mount is
/// exactly the one an Overwrite would empty.
fn mount_paths() -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut name = [0u16; GUID_PATH_LEN];
    // SAFETY: `name` holds `GUID_PATH_LEN` units.
    let handle = unsafe { FindFirstVolumeW(name.as_mut_ptr(), GUID_PATH_LEN as u32) };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let complete = loop {
        let Some(paths) = volume_path_names(&name) else {
            break false;
        };
        out.extend(paths);
        // SAFETY: `handle` came from `FindFirstVolumeW`; `name` holds `GUID_PATH_LEN` units.
        if unsafe { FindNextVolumeW(handle, name.as_mut_ptr(), GUID_PATH_LEN as u32) } == 0 {
            let code = std::io::Error::last_os_error().raw_os_error();
            break code.and_then(|code| u32::try_from(code).ok()) == Some(ERROR_NO_MORE_FILES);
        }
    };
    // SAFETY: `handle` came from `FindFirstVolumeW` and is closed once, here, on every path.
    unsafe { FindVolumeClose(handle) };
    complete.then_some(out)
}

/// Every path the volume GUID path in `name` is mounted at (`C:\`, `C:\Mnt\Data\`; none for
/// a volume mounted nowhere), or `None` when Windows wouldn't say.
fn volume_path_names(name: &[u16]) -> Option<Vec<String>> {
    let mut buffer = vec![0u16; 1024];
    let mut needed = 0u32;
    for _ in 0..3 {
        // SAFETY: `name` is NUL-terminated (the buffer `FindFirstVolumeW` filled), `buffer`
        // holds its length in units, and `needed` is a valid out pointer.
        let ok = unsafe {
            GetVolumePathNamesForVolumeNameW(
                name.as_ptr(),
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(u32::MAX),
                &raw mut needed,
            )
        };
        if ok != 0 {
            let end = (needed as usize).min(buffer.len());
            return Some(
                String::from_utf16_lossy(&buffer[..end])
                    .split('\0')
                    .filter(|path| !path.is_empty())
                    .map(str::to_string)
                    .collect(),
            );
        }
        let code = std::io::Error::last_os_error().raw_os_error();
        let more = code.and_then(|code| u32::try_from(code).ok()) == Some(ERROR_MORE_DATA);
        if !more || needed as usize <= buffer.len() {
            return None;
        }
        buffer = vec![0u16; needed as usize];
    }
    None
}

/// The folder mount `path` (`C:\mnt\data\`) as `std::fs::canonicalize` would spell the folder
/// itself: verbatim, in its on-disk case, read from the reparse point without following it
/// (following lands on the mounted volume's root). Falls back to the listed text verbatim.
fn folder_mount_as_canonical(path: &str) -> PathBuf {
    let trimmed = path.trim_end_matches('\\');
    let listed = || PathBuf::from(format!(r"\\?\{}", uppercase_drive(trimmed)));
    let Ok(folder) = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(trimmed)
    else {
        return listed();
    };
    let mut buffer = vec![0u16; 1024];
    for _ in 0..2 {
        let len = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open for this whole call (`folder` owns it), and `buffer`
        // holds `len` units; a larger answer is the length it needs, with nothing written.
        let written = unsafe {
            GetFinalPathNameByHandleW(
                folder.as_raw_handle(),
                buffer.as_mut_ptr(),
                len,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if written == 0 {
            return listed();
        }
        if written < buffer.len() {
            return PathBuf::from(OsString::from_wide(&buffer[..written]));
        }
        buffer = vec![0u16; written + 1];
    }
    listed()
}

/// A path as the calls here want it: backslashes, a trailing `\`, and no `\\?\` prefix.
struct Native {
    /// `C:\x\`, or for a share `\\server\share\x\`.
    plain: String,
    is_unc: bool,
}

impl Native {
    fn of(path: &Path) -> Self {
        let mut text = path.to_string_lossy().replace('/', "\\");
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            text = format!(r"\\{rest}");
        } else if let Some(rest) = text.strip_prefix(r"\\?\") {
            text = rest.to_string();
        }
        if !text.ends_with('\\') {
            text.push('\\');
        }
        let is_unc = text.starts_with(r"\\");
        Self {
            plain: uppercase_drive(&text),
            is_unc,
        }
    }

    /// `\\server\share\` with nothing below it.
    fn is_share_root(&self) -> bool {
        self.plain
            .trim_start_matches('\\')
            .trim_end_matches('\\')
            .split('\\')
            .count()
            <= 2
    }
}

/// The letter of a drive root (`C:\`, `c:\`), `None` for anything longer.
fn drive_root_letter(path: &str) -> Option<char> {
    let mut chars = path.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    (chars.as_str() == r":\").then(|| letter.to_ascii_uppercase())
}

/// `c:\mnt` as `C:\mnt`.
fn uppercase_drive(path: &str) -> String {
    let mut chars = path.chars();
    match (chars.next(), chars.clone().next()) {
        (Some(letter), Some(':')) => letter.to_ascii_uppercase().to_string() + chars.as_str(),
        _ => path.to_string(),
    }
}

/// `text` as a NUL-terminated wide string.
fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spelling_reads_as_one_native_path() {
        for spelling in [r"C:/", r"c:\", r"\\?\C:\", r"\\?\C:"] {
            assert_eq!(Native::of(Path::new(spelling)).plain, r"C:\", "{spelling}");
        }
        assert_eq!(Native::of(Path::new(r"\\?\c:\Mnt\Data")).plain, r"C:\Mnt\Data\");
        let share = Native::of(Path::new(r"\\?\UNC\srv\share"));
        assert!(share.is_unc && share.is_share_root());
        assert_eq!(share.plain, r"\\srv\share\");
        let below = Native::of(Path::new("//srv/share/x"));
        assert!(below.is_unc && !below.is_share_root());
    }

    #[test]
    fn only_a_bare_root_has_a_drive_letter() {
        assert_eq!(drive_root_letter(r"d:\"), Some('D'));
        assert_eq!(drive_root_letter(r"D:\x\"), None);
        assert_eq!(drive_root_letter(r"\\srv\share\"), None);
    }
}
