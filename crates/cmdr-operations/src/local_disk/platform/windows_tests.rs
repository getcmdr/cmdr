//! The Windows write probe against real NTFS: what it allows, what it refuses, and
//! the sharing violation it can't read as either.

use std::os::windows::fs::OpenOptionsExt;

use super::{WriteProbe, write_probe};

#[test]
fn a_fresh_folder_and_a_file_in_it_take_writes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plain.txt");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(write_probe(dir.path()), WriteProbe::Allowed);
    assert_eq!(write_probe(&file), WriteProbe::Allowed);
}

#[test]
fn a_read_only_file_is_refused_and_left_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("locked.txt");
    std::fs::write(&file, b"keep me").unwrap();
    let mut permissions = std::fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file, permissions.clone()).unwrap();

    assert!(
        matches!(write_probe(&file), WriteProbe::Refused(5)),
        "ERROR_ACCESS_DENIED"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"keep me");

    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "on Windows this only clears the read-only attribute, so the scratch folder can go"
    )]
    permissions.set_readonly(false);
    std::fs::set_permissions(&file, permissions).unwrap();
}

#[test]
fn a_missing_path_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(write_probe(&dir.path().join("nope")), WriteProbe::Refused(_)));
}

#[test]
fn a_file_another_handle_holds_unshared_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("held.txt");
    std::fs::write(&file, b"x").unwrap();
    let _held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    assert_eq!(write_probe(&file), WriteProbe::Unknown);
}

#[test]
fn a_folder_past_max_path_is_probed_whole() {
    let dir = tempfile::tempdir().unwrap();
    let mut deep = dir.path().to_path_buf();
    while deep.as_os_str().len() < 300 {
        deep.push("a_long_folder_name_for_the_probe");
    }
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(write_probe(&deep), WriteProbe::Allowed);
}
