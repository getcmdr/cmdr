//! Links on the Windows local disk: reading a symlink's or a junction's target, telling them
//! from the reparse points that are the item itself (`super::super::reparse`), and making a
//! symlink, a file link or a folder link by what its target is.
//!
//! ❗ Making a symlink takes Developer Mode or an elevated token; Cmdr never elevates, so
//! without either the OS refuses (`ERROR_PRIVILEGE_NOT_HELD`) and the volume answers
//! `NotSupported`, which the engine counts as "this destination holds no links"
//! (`LinksNotSupported`). A junction needs no privilege, but it's a different thing (absolute
//! targets only, folders only, resolved on the server for a share), so a link is never quietly
//! made one.

use std::io;
use std::path::{Component, Path, PathBuf};

use cmdr_fs::volume::VolumeError;

use super::super::reparse::ReparseKind;
pub(crate) use super::super::reparse::holds_no_links;

/// The kind of reparse point at `os` itself, never followed: `GetFileInformationByHandleEx`'s
/// `FileAttributeTagInfo` on a handle that opens the reparse point (and, with backup semantics,
/// a folder) asking only to read attributes.
pub(crate) fn reparse_kind_at(os: &Path) -> io::Result<ReparseKind> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo, GetFileInformationByHandleEx,
    };

    let handle = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(os)?;
    let mut info = FILE_ATTRIBUTE_TAG_INFO {
        FileAttributes: 0,
        ReparseTag: 0,
    };
    // SAFETY: `handle` is open for the call's duration, and `info` is the
    // `FILE_ATTRIBUTE_TAG_INFO` `FileAttributeTagInfo` fills, passed with its exact size.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut info).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).expect("a small struct"),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ReparseKind::of(info.FileAttributes, info.ReparseTag))
}

/// The target of the link at `os`, as stored: `Ok(None)` for anything but a symlink or a
/// junction (a placeholder or an alias included), `NotSupported` for a link Win32 can't read
/// (a WSL symlink). A junction's target is absolute, spelled as `std` hands it back
/// (`C:\x`, the NT `\??\` prefix dropped); a relative target comes back with `/`
/// (`carried_spelling`).
pub(crate) fn read_link_at(os: &Path) -> io::Result<Result<Option<PathBuf>, VolumeError>> {
    match reparse_kind_at(os)? {
        ReparseKind::Symlink | ReparseKind::Junction => Ok(Ok(Some(carried_spelling(std::fs::read_link(os)?)))),
        ReparseKind::OtherLink(tag) => {
            log::debug!(target: "local_windows", "read_link_target: a link with reparse tag {tag:#x} reads as unsupported");
            Ok(Err(VolumeError::NotSupported))
        }
        ReparseKind::None | ReparseKind::NotALink(_) => Ok(Ok(None)),
    }
}

/// Makes `link` a symlink holding `target` verbatim but for its separators (`stored_spelling`;
/// `read_link_at` hands a relative one back with `/`): a folder link when the target, read
/// against the link's folder, is a folder now, a file link otherwise (a target that's missing
/// included), the rule `cmdr_fs::testing::symlink` follows too. An occupied `link` is refused
/// by the OS itself (`ERROR_ALREADY_EXISTS`).
pub(crate) fn create_symlink_at(link: &Path, target: &Path) -> io::Result<()> {
    let target = stored_spelling(target);
    if target_is_folder(link, &target) {
        std::os::windows::fs::symlink_dir(&target, link)
    } else {
        std::os::windows::fs::symlink_file(&target, link)
    }
}

/// A target as the link stores it: every `/` a `\`. **Why**: Windows resolves a link's content
/// with `\` alone, so a relative `../lib/x` carried from macOS or Linux would land a link that
/// opens nothing (Go's `os.Symlink` converts the same way, "'/' does not work in link's
/// content", and so does Git for Windows). Only the separator changes; the target is still
/// never resolved or rebased.
fn stored_spelling(target: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    let wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .map(|unit| {
            if unit == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                unit
            }
        })
        .collect();
    PathBuf::from(std::ffi::OsString::from_wide(&wide))
}

/// A stored target as the volume hands it out: a RELATIVE (or rooted, drive-less) target with
/// `/` separators, so a link carried to macOS or Linux resolves there, and a target with a
/// prefix (`C:\x`, `\\server\share`, a junction's `\\?\`) exactly as stored, since only Windows
/// can open it and a verbatim prefix must keep its `\`.
fn carried_spelling(target: PathBuf) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    if matches!(target.components().next(), Some(Component::Prefix(_))) {
        return target;
    }
    let wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .map(|unit| {
            if unit == u16::from(b'\\') {
                u16::from(b'/')
            } else {
                unit
            }
        })
        .collect();
    PathBuf::from(std::ffi::OsString::from_wide(&wide))
}

/// The Windows arm of `crate::remove_leaf`: the entry at `path` itself, by its own attributes
/// (`remove_leaf_of`). `path` in either spelling (`C:\x`, `C:/x`); it goes through its verbatim
/// form, so a path past `MAX_PATH` removes too. A path with no app spelling (a name that isn't
/// Unicode text) is removed as given, the way the walker's `remove_file` always took it: a
/// leaf the walk found is never refused for its spelling.
pub fn remove_leaf(path: &Path) -> io::Result<()> {
    let os = super::paths::app_of(path)
        .and_then(|app| super::paths::native_of(&app))
        .unwrap_or_else(|_| path.to_path_buf());
    remove_leaf_of(&os, &std::fs::symlink_metadata(&os)?)
}

/// Removes the entry at the OS path `os` itself, never what it points at. A folder link (a
/// junction, a folder symlink) carries `FILE_ATTRIBUTE_DIRECTORY` on the link itself, and only
/// `RemoveDirectoryW` removes it: that takes the link and never its target's contents, and
/// works on a broken one too. `DeleteFileW` refuses one with `ERROR_ACCESS_DENIED`, which once
/// made a permanent delete of a junction fail as "Access is denied" (#1386). Anything else is
/// a file, a file symlink included.
pub(crate) fn remove_leaf_of(os: &Path, metadata: &std::fs::Metadata) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
    if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
        std::fs::remove_dir(os)
    } else {
        std::fs::remove_file(os)
    }
}

/// Whether `target` names a folder, read the way Windows resolves a relative link: against the
/// link's own folder, `..` by text. Lexical on purpose: `link` is a verbatim `\\?\` path, which
/// Win32 never normalizes, so a plain join would keep a literal `..`.
fn target_is_folder(link: &Path, target: &Path) -> bool {
    let mut resolved = match link.parent() {
        Some(folder) if target.is_relative() && !target.has_root() => folder.to_path_buf(),
        _ => PathBuf::new(),
    };
    for part in target.components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    std::fs::metadata(resolved).is_ok_and(|meta| meta.is_dir())
}
