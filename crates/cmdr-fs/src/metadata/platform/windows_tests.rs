//! A FILE's read-only attribute as Windows' Locked, against real NTFS: what a move's scan
//! reads, what it clears for a Move anyway, and what goes back on where the item lands. A
//! folder's read-only bit is Explorer's customization marker, so it's never a lock.

use std::path::Path;

use super::{add_flags, clear_blockers, removal_lock};
use crate::metadata::{ClassOutcome, LandedMetadata, RemovalLock, Unblock, UserFlags, apply_landed};

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).expect("the fixture exists").permissions();
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "on Windows this only sets or clears the read-only attribute"
    )]
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).expect("the fixture's attribute changes");
}

fn is_read_only(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .expect("the fixture exists")
        .permissions()
        .readonly()
}

#[test]
fn a_read_only_file_is_locked_and_a_read_only_folder_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("frozen.txt");
    std::fs::write(&file, b"x").unwrap();
    let folder = dir.path().join("customized");
    std::fs::create_dir(&folder).unwrap();
    assert_eq!(removal_lock(&file), RemovalLock::Free, "a plain file is free");

    set_read_only(&file, true);
    set_read_only(&folder, true);

    assert_eq!(removal_lock(&file), RemovalLock::Locked);
    assert_eq!(
        removal_lock(&folder),
        RemovalLock::Free,
        "a folder's read-only bit is Explorer's customization marker, not a lock"
    );
    assert_eq!(removal_lock(&dir.path().join("missing")), RemovalLock::Free);
    set_read_only(&file, false);
    set_read_only(&folder, false);
}

/// The forward-slash spelling (`C:/x`) a volume walk carries reaches the item too.
#[test]
fn the_app_spelling_reads_the_same_lock() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("frozen.txt");
    std::fs::write(&file, b"x").unwrap();
    set_read_only(&file, true);
    let forward_slashes = std::path::PathBuf::from(file.to_str().unwrap().replace('\\', "/"));

    assert_eq!(removal_lock(&forward_slashes), RemovalLock::Locked);
    set_read_only(&file, false);
}

#[test]
fn clearing_hands_back_locked_and_putting_it_back_locks_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("frozen.txt");
    std::fs::write(&file, b"keep me").unwrap();
    set_read_only(&file, true);

    let Unblock::Cleared(cleared) = clear_blockers(&file) else {
        panic!("a read-only file has its attribute cleared");
    };
    assert_eq!(cleared.flags(), UserFlags::LOCKED);
    assert!(!is_read_only(&file), "cleared on disk");
    assert_eq!(removal_lock(&file), RemovalLock::Free);

    let report = apply_landed(&file, &cleared);
    assert!(!report.any_refused(), "{report:?}");
    assert!(is_read_only(&file), "and back on");
    assert_eq!(std::fs::read(&file).unwrap(), b"keep me");
    set_read_only(&file, false);
}

#[test]
fn a_plain_file_and_a_read_only_folder_have_nothing_to_clear() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plain.txt");
    std::fs::write(&file, b"x").unwrap();
    let folder = dir.path().join("customized");
    std::fs::create_dir(&folder).unwrap();
    set_read_only(&folder, true);

    assert_eq!(clear_blockers(&file), Unblock::NothingToClear);
    assert_eq!(clear_blockers(&folder), Unblock::NothingToClear);
    assert!(is_read_only(&folder), "a folder's marker stays as it was");
    assert_eq!(clear_blockers(&dir.path().join("missing")), Unblock::NothingToClear);
    set_read_only(&folder, false);
}

fn is_hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    std::fs::symlink_metadata(path)
        .expect("the fixture exists")
        .file_attributes()
        & 0x2
        != 0
}

#[test]
fn locked_goes_on_a_file_and_hidden_on_anything() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plain.txt");
    std::fs::write(&file, b"x").unwrap();
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder).unwrap();

    assert_eq!(add_flags(&file, UserFlags::HIDDEN), ClassOutcome::Kept);
    assert!(is_hidden(&file) && !is_read_only(&file), "Hidden alone, nothing more");
    assert_eq!(add_flags(&folder, UserFlags::LOCKED), ClassOutcome::NotApplicable);
    assert!(!is_read_only(&folder), "a folder never takes it as a lock");
    assert_eq!(
        add_flags(&folder, UserFlags::HIDDEN | UserFlags::LOCKED),
        ClassOutcome::Kept
    );
    assert!(is_hidden(&folder) && !is_read_only(&folder));
    assert_eq!(add_flags(&file, UserFlags::NO_DUMP), ClassOutcome::NotApplicable);
    let set = LandedMetadata::default().with_flags(UserFlags::LOCKED);
    assert!(!apply_landed(&file, &set).any_refused());
    assert!(is_read_only(&file) && is_hidden(&file), "added to what it wore");
    set_read_only(&file, false);
}

/// What a copy reads off its source: the file's read-only and hidden attributes, and a
/// folder's hidden one (its read-only bit is no lock).
#[test]
fn a_copy_reads_locked_and_hidden_off_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("frozen.txt");
    std::fs::write(&file, b"x").unwrap();
    let folder = dir.path().join("customized");
    std::fs::create_dir(&folder).unwrap();
    assert_eq!(super::flags_of(&std::fs::metadata(&file).unwrap()), UserFlags::NONE);

    set_read_only(&file, true);
    assert_eq!(add_flags(&file, UserFlags::HIDDEN), ClassOutcome::Kept);
    set_read_only(&folder, true);
    assert_eq!(add_flags(&folder, UserFlags::HIDDEN), ClassOutcome::Kept);

    assert_eq!(
        super::flags_of(&std::fs::metadata(&file).unwrap()),
        UserFlags::LOCKED | UserFlags::HIDDEN
    );
    assert_eq!(super::flags_of(&std::fs::metadata(&folder).unwrap()), UserFlags::HIDDEN);
    set_read_only(&file, false);
    set_read_only(&folder, false);
}
