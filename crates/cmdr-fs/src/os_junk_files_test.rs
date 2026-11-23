use std::ffi::OsStr;

use super::{JunkOs, OsJunkFiles, is_os_junk_file_name};

fn is_junk(os: JunkOs, name: &str) -> bool {
    is_os_junk_file_name(os, OsStr::new(name))
}

#[test]
fn macos_skips_finder_metadata_and_apple_double_files() {
    for name in [".DS_Store", "._photo.jpg", "._x", "._.DS_Store"] {
        assert!(is_junk(JunkOs::MacOs, name), "{name} is Mac junk");
    }
}

#[test]
fn macos_look_alikes_stay_ordinary_files() {
    for name in [
        "._",
        ".DS_Store.bak",
        ".ds_store",
        "DS_Store",
        "_photo.jpg",
        ".gitignore",
        "a._b",
        "Thumbs.db",
    ] {
        assert!(!is_junk(JunkOs::MacOs, name), "{name} is an ordinary file on a Mac");
    }
}

#[test]
fn windows_skips_explorer_and_media_center_thumbnail_caches_in_any_case() {
    for name in [
        "Thumbs.db",
        "thumbs.db",
        "THUMBS.DB",
        "ehthumbs.db",
        "EhThumbs.db",
        "ehthumbs_vista.db",
    ] {
        assert!(is_junk(JunkOs::Windows, name), "{name} is Windows junk");
    }
}

#[test]
fn windows_keeps_folder_settings_the_recycle_bin_and_look_alikes() {
    for name in [
        "desktop.ini",
        "Desktop.ini",
        "$RECYCLE.BIN",
        "System Volume Information",
        "Thumbs.db.bak",
        "my Thumbs.db",
        "Thumbs.dbx",
        ".DS_Store",
        "._photo.jpg",
    ] {
        assert!(!is_junk(JunkOs::Windows, name), "{name} is an ordinary file on Windows");
    }
}

#[test]
fn linux_skips_nfs_and_fuse_leftovers_of_deleted_open_files() {
    for name in [
        // `.nfs` + 16 hex digits of file id + 8 of counter (fs/nfs/unlink.c).
        ".nfs000000000012abcd00000001",
        // `.fuse_hidden` + two 8-digit hex numbers (libfuse lib/fuse.c).
        ".fuse_hidden0000000a00000001",
    ] {
        assert!(is_junk(JunkOs::Linux, name), "{name} is Linux junk");
    }
}

#[test]
fn linux_keeps_folder_settings_the_trash_and_look_alikes() {
    for name in [
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
        ".DS_Store",
        "Thumbs.db",
    ] {
        assert!(!is_junk(JunkOs::Linux, name), "{name} is an ordinary file on Linux");
    }
}

#[test]
fn every_os_matches_its_own_sample_names() {
    for os in JunkOs::ALL {
        for name in os.sample_junk_names() {
            assert!(is_junk(os, name), "{name} is {os:?} junk");
        }
    }
}

#[test]
fn the_running_os_picks_its_own_set() {
    let expected = match std::env::consts::OS {
        "macos" => JunkOs::MacOs,
        "windows" => JunkOs::Windows,
        _ => JunkOs::Linux,
    };
    assert_eq!(JunkOs::current(), expected);
}

#[test]
fn only_skip_leaves_anything_out() {
    for os in JunkOs::ALL {
        let junk = os.sample_junk_names()[0];
        assert!(OsJunkFiles::Skip.leaves_out_on(os, OsStr::new(junk), false));
        assert!(!OsJunkFiles::Include.leaves_out_on(os, OsStr::new(junk), false));
        assert!(!OsJunkFiles::Skip.leaves_out_on(os, OsStr::new("notes.txt"), false));
    }
}

#[test]
fn a_folder_never_matches() {
    for os in JunkOs::ALL {
        for name in os.sample_junk_names() {
            assert!(
                !OsJunkFiles::Skip.leaves_out_on(os, OsStr::new(name), true),
                "folder {name}"
            );
        }
    }
}

#[test]
fn leaves_out_asks_the_running_os() {
    let os = JunkOs::current();
    for name in os.sample_junk_names() {
        assert!(OsJunkFiles::Skip.leaves_out_str(name, false), "{name}");
    }
}

#[test]
fn only_skipping_the_mac_set_drops_apple_double_sidecars() {
    assert!(OsJunkFiles::Skip.leaves_out_apple_double_sidecars_on(JunkOs::MacOs));
    assert!(!OsJunkFiles::Skip.leaves_out_apple_double_sidecars_on(JunkOs::Windows));
    assert!(!OsJunkFiles::Skip.leaves_out_apple_double_sidecars_on(JunkOs::Linux));
    for os in JunkOs::ALL {
        assert!(!OsJunkFiles::Include.leaves_out_apple_double_sidecars_on(os));
    }
}

#[test]
fn travels_over_ipc_in_camel_case() {
    assert_eq!(serde_json::to_string(&OsJunkFiles::Skip).unwrap(), "\"skip\"");
    assert_eq!(
        serde_json::from_str::<OsJunkFiles>("\"include\"").unwrap(),
        OsJunkFiles::Include
    );
}
