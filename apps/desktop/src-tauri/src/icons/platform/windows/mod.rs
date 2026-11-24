//! Windows: the Shell draws every icon (`shell_image.rs`), from one STA per asking thread
//! (`apartment.rs`). Types are asked through Cmdr's own empty sample files, so a type lookup
//! never touches the user's file; a real place (a special folder, a program or shortcut with
//! its own icon) is asked only when `place.rs` says it stays on this machine and holds its
//! bytes, and a shortcut only when both places it names do (`shortcut.rs`).

mod apartment;
mod place;
mod shell_image;
mod shortcut;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use image::DynamicImage;

use super::super::image_to_data_url;
use super::PathIcons;
use crate::config::ICON_SIZE;

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

/// The place's own icon, or for a shortcut pointing past this machine its type's icon.
fn own_icon(native: &Path) -> Option<String> {
    if !place::may_read(native) {
        return None;
    }
    if shortcut::is_shortcut(native) && !shortcut::points_only_here(native) {
        return crate::icons::icon_sample_path("lnk").and_then(|sample| fetch_icon_for_path(&sample));
    }
    fetch_icon_for_path(native)
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

/// No reader yet (an NTFS alternate data stream is where these would live), so a folder never
/// reads as custom-iconed.
pub(crate) fn extended_attribute(_path: &Path, _name: &str) -> Option<Vec<u8>> {
    None
}
