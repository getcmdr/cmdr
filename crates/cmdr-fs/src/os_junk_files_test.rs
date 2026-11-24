use std::ffi::OsStr;

use super::{OsJunkFiles, RUNNING_ON_MACOS, SAMPLE_JUNK_NAMES, is_os_junk_file_name};

fn is_junk(name: &str) -> bool {
    is_os_junk_file_name(OsStr::new(name))
}

#[test]
fn skips_finder_metadata_and_apple_double_files() {
    for name in [".DS_Store", "._photo.jpg", "._x", "._.DS_Store"] {
        assert!(is_junk(name), "{name} is Mac junk");
    }
}

#[test]
fn skips_explorer_and_media_center_thumbnail_caches_in_any_case() {
    for name in [
        "Thumbs.db",
        "thumbs.db",
        "THUMBS.DB",
        "ehthumbs.db",
        "EhThumbs.db",
        "ehthumbs_vista.db",
    ] {
        assert!(is_junk(name), "{name} is Windows junk");
    }
}

#[test]
fn skips_nfs_and_fuse_leftovers_of_deleted_open_files() {
    for name in [
        // `.nfs` + 16 hex digits of file id + 8 of counter (fs/nfs/unlink.c).
        ".nfs000000000012abcd00000001",
        // `.fuse_hidden` + two 8-digit hex numbers (libfuse lib/fuse.c).
        ".fuse_hidden0000000a00000001",
    ] {
        assert!(is_junk(name), "{name} is Linux junk");
    }
}

/// Folder settings, trash, and recovery folders are the person's (each OS's
/// `desktop.ini` and `.directory` carry a folder's custom icon and view), and a
/// look-alike name is somebody's own file.
#[test]
fn keeps_folder_settings_the_trash_and_look_alikes() {
    for name in [
        // macOS look-alikes
        "._",
        ".DS_Store.bak",
        ".ds_store",
        "DS_Store",
        "_photo.jpg",
        ".gitignore",
        "a._b",
        // Windows folder settings, trash, and look-alikes
        "desktop.ini",
        "Desktop.ini",
        "$RECYCLE.BIN",
        "System Volume Information",
        "Thumbs.db.bak",
        "my Thumbs.db",
        "Thumbs.dbx",
        // Linux folder settings, trash, and look-alikes
        ".directory",
        ".Trash-1000",
        "lost+found",
        ".nfs",
        ".nfsconfig",
        ".nfs000000000012abcd0000000",
        ".nfs000000000012abcd000000012",
        ".nfs000000000012ABCD00000001",
        ".nfs000000000012abcg00000001",
        ".fuse_hidden",
        ".fuse_hidden0000000a0000000",
        ".fuse_hidden0000000a00000001.bak",
    ] {
        assert!(!is_junk(name), "{name} is an ordinary file");
    }
}

#[test]
fn the_sample_names_are_junk() {
    for name in SAMPLE_JUNK_NAMES {
        assert!(is_junk(name), "{name}");
    }
}

#[test]
fn only_skip_leaves_anything_out() {
    for junk in SAMPLE_JUNK_NAMES {
        assert!(OsJunkFiles::Skip.leaves_out_str(junk, false));
        assert!(!OsJunkFiles::Include.leaves_out_str(junk, false));
    }
    assert!(!OsJunkFiles::Skip.leaves_out_str("notes.txt", false));
}

#[test]
fn a_folder_never_matches() {
    for name in SAMPLE_JUNK_NAMES {
        assert!(!OsJunkFiles::Skip.leaves_out_str(name, true), "folder {name}");
    }
}

#[test]
fn the_running_os_is_macos_only_on_a_mac() {
    assert_eq!(RUNNING_ON_MACOS, std::env::consts::OS == "macos");
}

#[test]
fn only_skipping_on_a_mac_drops_apple_double_sidecars() {
    assert!(OsJunkFiles::Skip.leaves_out_apple_double_sidecars_on(true));
    assert!(!OsJunkFiles::Skip.leaves_out_apple_double_sidecars_on(false));
    assert!(!OsJunkFiles::Include.leaves_out_apple_double_sidecars_on(true));
    assert!(!OsJunkFiles::Include.leaves_out_apple_double_sidecars_on(false));
    assert_eq!(OsJunkFiles::Skip.leaves_out_apple_double_sidecars(), RUNNING_ON_MACOS);
}

#[test]
fn travels_over_ipc_in_camel_case() {
    assert_eq!(serde_json::to_string(&OsJunkFiles::Skip).unwrap(), "\"skip\"");
    assert_eq!(
        serde_json::from_str::<OsJunkFiles>("\"include\"").unwrap(),
        OsJunkFiles::Include
    );
}
