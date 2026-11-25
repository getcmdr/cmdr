use std::path::Path;
use std::time::SystemTime;

/// `getxattr`: `None` for an absent attribute and for any read error (permission, dead mount).
pub(crate) fn extended_attribute(path: &Path, name: &str) -> Option<Vec<u8>> {
    xattr::get(path, name).ok().flatten()
}

/// No icon file of its own: a custom folder icon lives in extended attributes here.
pub(crate) fn draws_own_folder_icon(_path: &Path) -> bool {
    false
}

/// The place's own mtime: Finder bumps a folder's when it re-icons it.
pub(crate) fn icon_source_modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
