use std::path::Path;

use super::*;

/// The real table of the machine the tests run on: the root is always mounted, and a fresh
/// temp folder under it isn't a mount of its own.
#[test]
#[cfg(unix)]
fn the_table_lists_the_root_and_not_a_plain_folder() {
    let mounts = list().expect("the mount table reads");
    assert!(mounts.iter().any(|mount| mount == Path::new("/")), "{mounts:?}");
    assert_eq!(is_mount_point(Path::new("/")), Some(true));

    let dir = cmdr_fs::testing::TestDir::new("mounts_plain_folder");
    let plain = dir.join("plain");
    std::fs::create_dir(&plain).expect("plain folder");
    assert_eq!(is_mount_point(&plain), Some(false));
}

#[test]
fn a_user_mount_prefix_is_an_absolute_folder_path() {
    for prefix in cmdr_fs::mount_layout::user_mount_prefixes() {
        assert!(prefix.starts_with('/') && prefix.ends_with('/'), "{prefix}");
    }
}

/// Windows' table, on the real machine: the drive holding the temp folder is listed the way
/// `canonicalize` spells it, so the write engine's "a mount under this folder?" question
/// (`strip_prefix` of each mount against a resolved folder) finds the drive's root and nothing
/// under a plain folder.
#[test]
#[cfg(target_os = "windows")]
fn the_windows_table_lists_the_drive_root_as_canonicalize_spells_it() {
    let dir = cmdr_fs::testing::TestDir::new("mounts_windows_table");
    let resolved = dir.canonicalize().expect("the temp folder resolves");
    let drive_root = resolved.ancestors().last().expect("a root").to_path_buf();
    let mounts = list().expect("the mount table reads");
    assert!(mounts.contains(&drive_root), "{drive_root:?} not in {mounts:?}");
    assert!(
        mounts.iter().all(|mount| mount.strip_prefix(&resolved).is_err()),
        "no mount sits under a fresh temp folder: {mounts:?}"
    );
}

/// Each spelling of the drive root is a mount point; a plain folder, a file, a missing path,
/// and a junction (even one to a drive root) are not.
#[test]
#[cfg(target_os = "windows")]
fn a_windows_drive_root_is_a_mount_point_in_every_spelling_and_a_junction_is_not() {
    let dir = cmdr_fs::testing::TestDir::new("mounts_windows_points");
    let resolved = dir.canonicalize().expect("the temp folder resolves");
    let verbatim_root = resolved.ancestors().last().expect("a root").to_path_buf();
    let letter = verbatim_root.to_string_lossy().chars().nth(4).expect("a drive letter");
    for spelling in [
        format!("{letter}:/"),
        format!("{letter}:\\"),
        format!(r"\\?\{letter}:\"),
    ] {
        assert_eq!(is_mount_point(Path::new(&spelling)), Some(true), "{spelling}");
    }

    let plain = dir.join("plain");
    std::fs::create_dir(&plain).expect("plain folder");
    let file = dir.join("file.txt");
    std::fs::write(&file, b"x").expect("a file");
    let junction = dir.join("to-the-root");
    let made = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(format!("{letter}:\\"))
        .output()
        .expect("cmd runs");
    assert!(made.status.success(), "mklink /J: {made:?}");

    assert_eq!(is_mount_point(&plain), Some(false));
    assert_eq!(is_mount_point(&plain.canonicalize().expect("resolves")), Some(false));
    assert_eq!(is_mount_point(&file), Some(false));
    assert_eq!(is_mount_point(&dir.join("missing")), Some(false));
    assert_eq!(is_mount_point(&junction), Some(false));
    std::fs::remove_dir(&junction).expect("the junction goes, its target stays");
}

/// A folder on a share holds no local mount; the share's own root is the server's to answer.
#[test]
#[cfg(target_os = "windows")]
fn a_windows_share_folder_is_no_mount_and_its_root_cant_say() {
    assert_eq!(
        is_mount_point(Path::new(r"\\?\UNC\testnas.local\Share\photos")),
        Some(false)
    );
    assert_eq!(is_mount_point(Path::new("//testnas.local/Share/photos")), Some(false));
    assert_eq!(is_mount_point(Path::new(r"\\testnas.local\Share")), None);
}
