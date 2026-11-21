//! The Windows write probe against real NTFS: what it allows, what it refuses, and
//! the sharing violation it can't read as either. And the read-only attribute as
//! Locked: what a permanent delete and an Overwrite do with it.

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

fn set_read_only(path: &std::path::Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "on Windows this only sets or clears the read-only attribute"
    )]
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn a_read_only_file_is_locked_and_a_read_only_folder_is_not() {
    use super::{is_locked_item, locked_against_removal};

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("frozen.txt");
    std::fs::write(&file, b"x").unwrap();
    let folder = dir.path().join("customized");
    std::fs::create_dir(&folder).unwrap();
    assert!(!is_locked_item(&file), "a plain file is free");

    set_read_only(&file, true);
    set_read_only(&folder, true);

    assert!(is_locked_item(&file) && locked_against_removal(&file));
    assert!(
        !is_locked_item(&folder),
        "a folder's read-only bit is Explorer's customization marker, not a lock"
    );
    assert!(!locked_against_removal(&dir.path().join("missing")));
    set_read_only(&file, false);
    set_read_only(&folder, false);
}

/// `std::fs::remove_file` deletes a read-only file on Windows, so without the up-front
/// ask a permanent delete would walk straight through the lock.
#[test]
fn a_permanent_delete_refuses_a_read_only_file_as_locked_and_keeps_it() {
    use crate::delete::delete_files_with_progress_inner;
    use crate::event_sinks::CollectorEventSink;
    use crate::testing::TestOperations;
    use crate::types::{WriteOperationConfig, WriteOperationError};

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("frozen.txt");
    std::fs::write(&file, b"keep me").unwrap();
    set_read_only(&file, true);
    let events = CollectorEventSink::new();
    let engine = TestOperations::new();
    let state = std::sync::Arc::new(engine.new_operation_state(std::time::Duration::from_millis(0)));

    let result = delete_files_with_progress_inner(
        &events,
        "op-delete-read-only",
        &state,
        std::slice::from_ref(&file),
        &WriteOperationConfig::default(),
    );

    assert!(
        matches!(result, Err(WriteOperationError::FileLocked { .. })),
        "{result:?}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"keep me");
    set_read_only(&file, false);
}

/// The landing's rename would carry a read-only destination aside and delete it, so an
/// Overwrite refuses it as Locked before writing a byte.
#[test]
fn an_overwrite_refuses_a_read_only_destination_as_locked_and_keeps_it() {
    use crate::host::Announced;
    use crate::overwrite::{PostLanding, Staged, stage_and_land_file};
    use crate::testing::TestOperations;
    use crate::types::WriteOperationError;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("frozen.txt");
    std::fs::write(&dest, b"keep me").unwrap();
    set_read_only(&dest, true);
    let state = std::sync::Arc::new(TestOperations::new().new_operation_state(std::time::Duration::from_millis(0)));

    let result = stage_and_land_file(&state, Announced::assumed(&dest), true, PostLanding::Apply, |temp| {
        std::fs::write(temp, b"new").unwrap();
        Ok(Staged::bytes(3))
    });

    assert!(
        matches!(result, Err(WriteOperationError::FileLocked { .. })),
        "{:?}",
        result.err()
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"keep me");
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(names.len(), 1, "no temp or aside left: {names:?}");
    set_read_only(&dest, false);
}
