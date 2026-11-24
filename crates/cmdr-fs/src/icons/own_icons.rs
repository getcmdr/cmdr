//! Per-path icon keys for FILES that carry an icon of their own (Windows: a program's embedded
//! icon, a shortcut's target's, an icon file's picture), and the type icon that stands in when
//! the app may not read the place.
//!
//! Which files carry one is a name rule (`file_extension::has_own_icon`), pure, so
//! `entry::get_icon_id` routes a hit straight to its `path:` key during listing. The key shares
//! the `path:` lifecycle of custom-icon folders: unbounded, LRU-capped, never persisted to the
//! frontend's localStorage.

/// Prefix of the per-path icon keys (`path:C:/Tools/tool.exe`, a custom-icon folder's
/// `path:/Users/x/Projects`): the icon belongs to the thing at that path.
pub const PATH_KEY_PREFIX: &str = "path:";

/// The `path:{path}` key for the file at `path`, whose icon is its own.
pub fn own_icon_id(path: &str) -> String {
    format!("{PATH_KEY_PREFIX}{path}")
}

/// The type icon to show for a real-place icon id (`path:`, `pkg:`, `special:`) whose place
/// the app may not read for its own icon (a network share, a cloud placeholder, inside an
/// archive): the file's extension icon for a file that would carry its own (`ext:exe`), else
/// the generic folder, since every other real-place id names a folder.
pub fn stand_in_icon_id(icon_id: &str) -> String {
    // App text separates names with `/` on every OS, and the last name is all this reads.
    let file_name = icon_id
        .strip_prefix(PATH_KEY_PREFIX)
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| crate::file_extension::has_own_icon(name));
    match file_name.and_then(|name| name.rsplit_once('.')) {
        Some((_, ext)) => format!("ext:{}", ext.to_lowercase()),
        None => "dir".to_string(),
    }
}

/// Shortcut kinds whose icon belongs to another file (the target, the deployed app), so the
/// shortcut's own mtime can't say when its icon changed.
const POINTS_ELSEWHERE: &[&str] = &["lnk", "appref-ms"];

/// Whether a real-place icon id may live in the on-disk tier, whose staleness token is the
/// place's own mtime: every one but a shortcut's (`path:…/Docs.lnk`), whose icon follows a
/// file the token can't see.
pub fn keeps_on_disk(icon_id: &str) -> bool {
    let name = icon_id
        .strip_prefix(PATH_KEY_PREFIX)
        .and_then(|path| path.rsplit('/').next());
    !name
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(_, ext)| POINTS_ELSEWHERE.iter().any(|kind| ext.eq_ignore_ascii_case(kind)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_stays_out_of_the_disk_tier() {
        assert!(!keeps_on_disk("path:C:/x/Docs.LNK"));
        assert!(!keeps_on_disk("path:C:/x/app.appref-ms"));
        assert!(keeps_on_disk("path:C:/x/tool.exe"));
        assert!(keeps_on_disk("path:/Users/x/Custom Folder"));
        assert!(keeps_on_disk("special:downloads"));
        assert!(keeps_on_disk("pkg:/Applications/Safari.app"));
    }

    #[test]
    fn own_icon_id_uses_the_path_prefix_and_full_path() {
        assert_eq!(own_icon_id("C:/Tools/tool.exe"), "path:C:/Tools/tool.exe");
    }

    #[test]
    fn folder_ids_stand_in_as_the_generic_folder() {
        assert_eq!(stand_in_icon_id("special:downloads"), "dir");
        assert_eq!(stand_in_icon_id("pkg:/Applications/Safari.app"), "dir");
        assert_eq!(stand_in_icon_id("path:/Users/x/Custom Folder"), "dir");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn an_own_icon_file_stands_in_as_its_extension_icon() {
        assert_eq!(stand_in_icon_id("path://nas/share/Setup.EXE"), "ext:exe");
        assert_eq!(stand_in_icon_id("path:Z:/Links/Docs.lnk"), "ext:lnk");
        assert_eq!(stand_in_icon_id("path:C:/x/app.appref-ms"), "ext:appref-ms");
    }
}
