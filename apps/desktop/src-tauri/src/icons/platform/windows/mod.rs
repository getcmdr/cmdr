//! Windows: the Shell draws every icon (`shell_image.rs`), from one STA per asking thread
//! (`platform/windows/apartment.rs`). Types are asked through Cmdr's own empty sample files, so a type lookup
//! never touches the user's file; a real place (a special folder, a program or shortcut with
//! its own icon) is asked only when `place.rs` says it stays on this machine and holds its
//! bytes, a shortcut only when both places it names do (`shortcut.rs`), and a customized folder
//! only when its `desktop.ini` names a local icon (`desktop_ini.rs`). A link carries the
//! system's shortcut arrow, and a drive the Shell mustn't be asked about its stock icon
//! (`overlay.rs`).

mod desktop_ini;
mod overlay;
mod place;
mod shell_image;
mod shortcut;

#[cfg(test)]
mod desktop_ini_tests;
#[cfg(test)]
mod overlay_tests;
#[cfg(test)]
mod tests;

pub(crate) use shell_image::app_icon_at;

/// Whether an icon location stays on this machine and holds its bytes (`place.rs`), for
/// `shell_image::app_icon_at`.
pub(super) fn may_read_icon_location(path: &Path) -> bool {
    place::may_read(path)
}

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use image::DynamicImage;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

use super::super::image_to_data_url;
use super::PathIcons;
use crate::config::ICON_SIZE;
pub(crate) use overlay::{StockDriveIcon, stock_drive_icon};

/// Stack for the system drive's icon lookup: a Shell handler runs inside it, as on the icon
/// threads.
pub(crate) const PLACE_LOOKUP_STACK: usize = 8 * 1024 * 1024;

/// The Shell's icon for `path`, ungated: callers here pass Cmdr's own samples, which may sit
/// anywhere `%TEMP%` does (a network home folder included) and never touch a user's file.
/// A real place goes through [`fetch_path_icons`], which gates it.
pub(crate) fn fetch_icon_for_path(path: &Path) -> Option<String> {
    let img = shell_image::render(path, ICON_SIZE)?;
    image_to_data_url(&DynamicImage::ImageRgba8(img))
}

/// On the 8 MB icon threads: a third-party icon handler or a cloud filter runs inside the
/// lookup, on our stack. Each place is gated again here (`get_icons` already stood the
/// unreadable ones in; `refresh_directory_icons` didn't).
pub(crate) fn fetch_path_icons(paths: Vec<String>) -> PathIcons {
    super::icon_threads::fetch_on_icon_threads(paths, |path| {
        crate::app_text::native_plain(path).and_then(|native| own_icon(&native))
    })
}

/// Each native program path's own icon, gated as a place is, asked on the 8 MB icon threads (their
/// own STAs, kept by those threads), so a caller on a tokio pool thread never joins an apartment
/// it can't serve. For the settings rows that show a program (a terminal, a picked editor).
pub(crate) fn native_icons(paths: Vec<PathBuf>) -> Vec<Option<String>> {
    let texts = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
    super::icon_threads::fetch_on_icon_threads(texts, |text| own_icon(&PathBuf::from(text)))
        .into_iter()
        .map(|(_, icon)| icon)
        .collect()
}

/// The place's own icon, or for a shortcut pointing past this machine its type's icon.
///
/// - A plain folder (not a special one, not a drive root) is ❌ never handed to the Shell: its
///   icon is extracted from the one icon file its `desktop.ini` names, once vetted
///   (`desktop_ini.rs`), else `None`, so the frontend keeps the generic folder.
/// - A drive root is asked only for the system drive: any other one (an external disk, a mounted
///   VHDX, both "fixed") could carry an `autorun.inf` or `desktop.ini` naming a share.
/// - A special folder (Documents and friends) asks the Shell, from its known location.
fn own_icon(native: &Path) -> Option<String> {
    let attributes = place::local_attributes(native)?;
    if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        if native.parent().is_none() {
            if !is_system_drive_root(native) {
                return None;
            }
        } else if is_plain_folder(native) {
            let img = desktop_ini::folder_icon(native, ICON_SIZE)?;
            return image_to_data_url(&DynamicImage::ImageRgba8(img));
        }
    }
    if shortcut::is_shortcut(native) && !shortcut::points_only_here(native) {
        return crate::icons::icon_sample_path("lnk").and_then(|sample| fetch_icon_for_path(&sample));
    }
    fetch_icon_for_path(native)
}

/// The Shell's icon for the system drive's root (its Windows logo), blocking. The caller runs
/// it on a thread of its own with an 8 MB stack, like every real-place lookup.
pub(crate) fn system_drive_icon() -> Option<String> {
    crate::app_text::native_plain(&cmdr_fs::mount_layout::boot_volume_root()).and_then(|root| own_icon(&root))
}

/// Whether native root `root` (`C:\`) is the system drive's.
fn is_system_drive_root(root: &Path) -> bool {
    crate::app_text::of_native(root)
        .is_some_and(|app| app.eq_ignore_ascii_case(&cmdr_fs::mount_layout::boot_volume_root()))
}

/// A folder that's neither a drive root (its icon comes with the drive) nor a special folder
/// (Documents and friends keep their `special:` icon).
fn is_plain_folder(native: &Path) -> bool {
    native.parent().is_some() && crate::icons::special_folders::classify(native).is_none()
}

/// A folder marked customized whose `desktop.ini` names an icon file Cmdr vetted. A special
/// folder answers no: it already shows its `special:` icon.
pub(crate) fn draws_own_folder_icon(path: &Path) -> bool {
    is_plain_folder(path) && desktop_ini::has_vetted_icon(path)
}

/// The later of the place's mtime and, for a folder, its `desktop.ini`'s: Explorer's Customize
/// tab rewrites that file in place, which leaves the folder's own mtime alone.
pub(crate) fn icon_source_modified(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    let own = meta.modified().ok()?;
    if !meta.is_dir() {
        return Some(own);
    }
    let ini = std::fs::metadata(path.join("desktop.ini"))
        .and_then(|m| m.modified())
        .ok();
    Some(ini.map_or(own, |ini| ini.max(own)))
}

/// The generic file ids, through an extensionless sample: the Unix arms' `/etc/hosts` has no
/// Windows twin.
pub(crate) fn icon_for_id(icon_id: &str, _use_app_icons_for_documents: bool) -> Option<String> {
    match icon_id {
        "file" | "symlink-file" | "symlink" => generic_file_sample().and_then(|sample| fetch_icon_for_path(&sample)),
        _ => None,
    }
}

/// An empty, extensionless `<temp>/cmdr-icon-samples/sample-file`, beside the per-extension
/// samples: the Shell draws it as a file of no known type.
fn generic_file_sample() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("cmdr-icon-samples");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("sample-file");
    if !path.exists() {
        std::fs::File::create(&path).ok()?;
    }
    Some(path)
}

/// Whether app path `path` names a place on a local drive that holds its bytes here. Cheap and
/// COM-free; a shortcut's own places are checked at fetch time, on the icon thread.
pub(crate) fn may_read_own_icon(path: &str) -> bool {
    crate::app_text::native_plain(path).is_some_and(|native| place::may_read(&native))
}

/// The Shell keys a type's icon by its registered class, which `get_icons` reaches through
/// the sample file; there's no fresher source to read past it.
pub(crate) fn fresh_extension_icon(_ext: &str, _use_app_icons_for_documents: bool) -> Option<DynamicImage> {
    None
}

/// No object pool to drain: each asking thread keeps its own COM apartment.
pub(crate) fn with_object_pool<T, F: FnOnce() -> T>(f: F) -> T {
    f()
}

/// No Finder tags.
pub(crate) fn tag_tinting_disabled() -> bool {
    false
}

/// No reader yet (an NTFS alternate data stream is where these would live); a customized
/// folder is read from its `desktop.ini` instead ([`draws_own_folder_icon`]).
pub(crate) fn extended_attribute(_path: &Path, _name: &str) -> Option<Vec<u8>> {
    None
}
