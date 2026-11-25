//! A folder customized through its `desktop.ini` (an icon picked in Properties > Customize, a
//! folder a program set up with its own icon). Threat model: a listed folder is attacker-shaped
//! input. An extracted archive (7-Zip restores the read-only and system attributes) or a mounted
//! VHDX (a "fixed" drive) can hold a `desktop.ini` naming `\\host\…`, and a Shell that draws it
//! opens an SMB connection on listing, which can offer the user's NTLM credentials.
//!
//! So Cmdr never asks the Shell about such a folder. It reads the file itself (as
//! `GetPrivateProfileString` would: the first `[.ShellClassInfo]` section, the first of each
//! key), refuses anything it can't vouch for, vets the one icon file it names, and extracts the
//! icon from THAT file (`overlay::extract_icon`). No second reader sees the file, so none can
//! decide differently. Anything refused shows the plain folder icon. `DETAILS.md` §
//! "`desktop.ini` folders".

use std::io::Read;
use std::os::windows::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use image::RgbaImage;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Windows, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::GUID;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM,
};

/// Largest `desktop.ini` read; a real one is a few hundred bytes. A larger file is refused
/// whole: a key past the cap would be one the Shell sees and Cmdr doesn't.
pub(super) const MAX_LEN: u64 = 64 * 1024;

/// Keys that hand the folder to a Shell handler (`CLSID2={0AFACED1-…}` makes a folder shortcut
/// whose `target.lnk` the Shell resolves), so a folder carrying one anywhere is never drawn.
const HANDLER_KEYS: &[&str] = &["clsid", "clsid2", "uiclsid"];

/// The icon file kinds Cmdr extracts from; any other name (a `.lnk`, a `.url`) is refused.
const ICON_FILE_KINDS: &[&str] = &["ico", "icl", "dll", "exe", "cpl", "ocx", "mun"];

/// What a `desktop.ini` names as the folder's icon.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Parsed {
    /// The icon file as written and its index (a resource id when negative).
    Icon { file: String, index: i32 },
    /// No icon named: the plain folder.
    None,
    /// A handler key, or a file Cmdr won't vouch for: the plain folder.
    Refused,
}

/// The vetted icon of the folder at native `folder`, or `None` for the plain folder icon.
pub(super) fn folder_icon(folder: &Path, size: u32) -> Option<RgbaImage> {
    let (file, index) = vetted_icon_file(folder)?;
    super::overlay::extract_icon(&file, index, size)
}

/// Whether [`folder_icon`] would draw an icon of the folder's own, without extracting it.
pub(super) fn has_vetted_icon(folder: &Path) -> bool {
    vetted_icon_file(folder).is_some()
}

/// The icon file and index the folder's `desktop.ini` names, once every check passed: a local
/// folder on a fixed drive (❌ never a removable one, like its drive row), marked customized,
/// with a local `desktop.ini` within the size cap, no handler key, and an icon file that
/// [`vet_icon_file`] allows.
fn vetted_icon_file(folder: &Path) -> Option<(PathBuf, i32)> {
    if !super::place::on_fixed_drive(folder) {
        return None;
    }
    let attributes = super::place::local_attributes(folder)?;
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 || attributes & (FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_SYSTEM) == 0
    {
        return None;
    }
    let ini = folder.join("desktop.ini");
    let ini_attributes = super::place::raw_attributes(&ini)?;
    if ini_attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
        || super::place::bytes_elsewhere(ini_attributes)
    {
        return None;
    }
    let Parsed::Icon { file, index } = parse(&read_text(&ini)?) else {
        return None;
    };
    Some((vet_icon_file(folder, &file)?, index))
}

/// The icon file `desktop.ini` names, as a local path Cmdr may read, or `None`. Two shapes
/// pass, nothing else:
/// - A relative name inside the folder itself (`icon.ico`, `art\icon.ico`), with no `..`, no
///   reparse point on any step from the folder to the file (a junction there could point at a
///   share, and even reading its attributes past it would reach it), and bytes held here.
/// - An absolute path (after `%VAR%`s) under the Windows folder or a Program Files folder, as the
///   known-folder API names them, ❌ never as the text does.
fn vet_icon_file(folder: &Path, file: &str) -> Option<PathBuf> {
    let expanded = PathBuf::from(super::shortcut::expand_env_vars(file, |name| std::env::var(name).ok()));
    if !has_icon_file_kind(&expanded) || expanded.components().any(|c| !is_plain_step(&c)) {
        return None;
    }
    if expanded.is_absolute() {
        let trusted = TRUSTED_ROOTS.iter().any(|root| is_under(&expanded, root));
        return (trusted && super::place::may_read(&expanded)).then_some(expanded);
    }
    if expanded.has_root() || expanded.components().next().is_none() {
        return None;
    }
    let mut step = folder.to_path_buf();
    for component in expanded.components() {
        step.push(component.as_os_str());
        let attributes = super::place::raw_attributes(&step)?;
        if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || super::place::bytes_elsewhere(attributes) {
            return None;
        }
    }
    (super::place::raw_attributes(&step)? & FILE_ATTRIBUTE_DIRECTORY == 0).then_some(step)
}

/// A path step that names a place: a drive prefix, the root, or a plain name, ❌ never `.` or `..`.
fn is_plain_step(component: &Component<'_>) -> bool {
    matches!(
        component,
        Component::Prefix(_) | Component::RootDir | Component::Normal(_)
    )
}

fn has_icon_file_kind(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ICON_FILE_KINDS.iter().any(|kind| ext.eq_ignore_ascii_case(kind)))
}

/// Whether `path` lies under `root`, comparing names the way NTFS does by default (ignoring case).
pub(super) fn is_under(path: &Path, root: &Path) -> bool {
    let mut names = path.components();
    root.components().all(|want| {
        names
            .next()
            .is_some_and(|got| got.as_os_str().eq_ignore_ascii_case(want.as_os_str()))
    })
}

/// The Windows folder and the Program Files folders, from the known-folder API.
static TRUSTED_ROOTS: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    [FOLDERID_Windows, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86]
        .iter()
        .filter_map(known_folder)
        .collect()
});

fn known_folder(id: &GUID) -> Option<PathBuf> {
    // SAFETY: `id` is a live `GUID`; no token is passed (the current user), as documented.
    let text = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    // SAFETY: the API hands back a NUL-terminated string this call owns; the native path stays
    // wide, as the OS spelled it.
    let path: Option<PathBuf> = Some(std::ffi::OsString::from_wide(unsafe { text.as_wide() }).into());
    // SAFETY: freeing the string the API allocated with the COM allocator, once.
    unsafe { CoTaskMemFree(Some(text.0.cast_const().cast())) };
    path
}

/// The file's text, refused past [`MAX_LEN`]: UTF-16 with its byte-order mark (what Explorer
/// writes for a non-ASCII name), else UTF-8 (an ANSI name past ASCII then names no place).
fn read_text(ini: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(ini)
        .ok()?
        .take(MAX_LEN + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (u64::try_from(bytes.len()).ok()? <= MAX_LEN).then(|| decode(&bytes))
}

pub(super) fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => {
            let units: Vec<u16> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            String::from_utf16_lossy(&units)
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Reads `desktop.ini` the way `GetPrivateProfileString` does: the FIRST `[.ShellClassInfo]`
/// section (names ignore case), the FIRST line of each key there (even an empty one, which
/// then names nothing), `;` comment lines skipped, a value's quotes dropped. `IconResource=`
/// (`path,index`) wins over `IconFile=` + `IconIndex=`. A handler key in any section refuses
/// the file.
pub(super) fn parse(text: &str) -> Parsed {
    let mut section_seen = false;
    let mut in_section = false;
    let mut resource: Option<String> = None;
    let mut icon_file: Option<String> = None;
    let mut icon_index: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            let is_ours = section.trim().eq_ignore_ascii_case(".ShellClassInfo");
            in_section = is_ours && !section_seen;
            section_seen |= is_ours;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if HANDLER_KEYS.contains(&key.as_str()) {
            return Parsed::Refused;
        }
        if !in_section {
            continue;
        }
        let value = value.trim().trim_matches('"').trim().to_string();
        let slot = match key.as_str() {
            "iconresource" => &mut resource,
            "iconfile" => &mut icon_file,
            "iconindex" => &mut icon_index,
            _ => continue,
        };
        if slot.is_none() {
            *slot = Some(value);
        }
    }
    if let Some(resource) = resource {
        let (file, index) = resource.rsplit_once(',').unwrap_or((resource.as_str(), "0"));
        return icon(file, index);
    }
    match icon_file {
        Some(file) => icon(&file, icon_index.as_deref().unwrap_or("0")),
        None => Parsed::None,
    }
}

fn icon(file: &str, index: &str) -> Parsed {
    let file = file.trim();
    if file.is_empty() {
        return Parsed::None;
    }
    match index.trim().parse::<i32>() {
        Ok(index) => Parsed::Icon {
            file: file.to_string(),
            index,
        },
        Err(_) => Parsed::Refused,
    }
}
