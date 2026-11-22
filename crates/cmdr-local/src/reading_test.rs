//! Tests for the local reader: `std::fs` listing, single-entry stat, and `FileEntry` construction,
//! on every OS.

use crate::reading::{AddedAt, entry_from_lstat, get_single_entry, list_directory_core, withhold_unclaimed_dates};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::SupportedDates;
use cmdr_fs::volume::host::VolumeHost;
use std::fs;

// ============================================================================
// Tests for list_directory_core
// ============================================================================

#[test]
fn test_list_directory_core_returns_core_entries() {
    let temp_dir = TestDir::new("core_test");

    let test_file = temp_dir.join("core_test.txt");
    fs::write(&test_file, "content").unwrap();

    let entries = list_directory_core(&temp_dir, VolumeHost::detached().listings()).unwrap();

    assert!(!entries.is_empty());
    let file_entry = entries.iter().find(|e| e.name == "core_test.txt").unwrap();

    // Core metadata should be present. The owner is the POSIX uid's name; Windows reads no
    // owner (a SID per entry, `reading/platform/windows.rs`).
    assert!(!file_entry.name.is_empty());
    assert!(!file_entry.path.is_empty());
    #[cfg(unix)]
    assert!(!file_entry.owner.is_empty());

    // The added date comes with the listing on APFS (the scratch dir's
    // filesystem), never on Linux or Windows.
    assert_eq!(file_entry.added_at.is_some(), cfg!(target_os = "macos"));
}

#[test]
fn test_list_directory_core_is_sorted() {
    let temp_dir = TestDir::new("sort_test");

    // Create files in non-alphabetical order
    fs::write(temp_dir.join("zebra.txt"), "").unwrap();
    fs::write(temp_dir.join("alpha.txt"), "").unwrap();
    fs::create_dir(temp_dir.join("a_dir")).unwrap();

    let entries = list_directory_core(&temp_dir, VolumeHost::detached().listings()).unwrap();

    // Directories should come first, then sorted alphabetically. The fixture dir is
    // ours alone, so the count is exact rather than a floor over someone's leftovers.
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, "a_dir");
    assert!(entries[0].is_directory);
}

// ============================================================================
// Tests for get_single_entry
// ============================================================================

#[test]
fn test_get_single_entry_file() {
    let temp_dir = TestDir::new("single_entry_test");

    let test_file = temp_dir.join("single_file.txt");
    fs::write(&test_file, "test content").unwrap();

    let entry = get_single_entry(&test_file).unwrap();

    assert_eq!(entry.name, "single_file.txt");
    assert!(!entry.is_directory);
    assert!(!entry.is_symlink);
    assert_eq!(entry.size, Some(12)); // "test content" is 12 bytes
}

#[test]
fn test_get_single_entry_directory() {
    let temp_dir = TestDir::new("single_dir_test");

    let entry = get_single_entry(&temp_dir).unwrap();

    assert!(entry.name.contains("cmdr_single_dir_test"));
    assert!(entry.is_directory);
    assert!(!entry.is_symlink);
    assert!(entry.size.is_none());
}

/// On macOS and Linux a watch opens and reports the path as it is: the Windows crossing
/// (`watch_os_path`, `listing_spelling_of`) is the identity here.
#[test]
#[cfg(unix)] // POSIX: the watch path is the identity here; `local_windows_test.rs` pins the `\\?\` crossing
fn a_watch_path_crosses_as_itself_on_posix() {
    use crate::reading::{listing_spelling_of, watch_os_path};

    let temp_dir = TestDir::new("reading_watch_paths");
    let file = temp_dir.join("a.txt");
    assert_eq!(watch_os_path(&temp_dir).unwrap(), temp_dir.to_path_buf());
    assert_eq!(listing_spelling_of(&file), file);
}

/// The Windows case check (`named_exactly`) leaves macOS's stat alone: on a case-insensitive
/// APFS volume another spelling still answers, as it always has.
#[cfg(target_os = "macos")] // APFS is case-insensitive by default; a Linux disk usually isn't.
#[test]
fn a_stat_on_macos_still_answers_for_another_spelling() {
    let temp_dir = TestDir::new("reading_case_spelling");
    fs::write(temp_dir.join("a.txt"), "x").unwrap();
    if fs::metadata(temp_dir.join("A.TXT")).is_ok() {
        assert!(get_single_entry(&temp_dir.join("A.TXT")).is_ok());
    }
}

#[test]
fn test_get_single_entry_nonexistent() {
    let temp_dir = TestDir::new("single_missing_test");
    let result = get_single_entry(&temp_dir.join("definitely_does_not_exist_12345"));
    assert!(result.is_err());
}

// ============================================================================
// Tests for `entry_from_lstat`, the per-entry builder both listers share
// ============================================================================

/// The entry at `dir/name` through `entry_from_lstat`, outside any volume root.
fn entry_at(dir: &std::path::Path, name: &str) -> Option<FileEntry> {
    let path = dir.join(name);
    entry_from_lstat(
        &path,
        crate::reading::Row::of(&path, None),
        &std::collections::HashSet::new(),
        AddedAt::Skip,
    )
    .ok()
}

#[test]
fn test_entry_from_lstat_returns_file_entry() {
    let temp_dir = TestDir::new("process_entry_test");
    fs::write(temp_dir.join("process_test.txt"), "test content").unwrap();

    let entry = entry_at(&temp_dir, "process_test.txt").expect("entry reads");
    assert_eq!(entry.name, "process_test.txt");
    assert!(!entry.is_directory);
    assert!(!entry.is_symlink);
    assert_eq!(entry.size, Some(12)); // "test content" is 12 bytes
}

#[test]
fn test_entry_from_lstat_handles_directory() {
    let temp_dir = TestDir::new("process_dir_test");
    fs::create_dir(temp_dir.join("sub_directory")).unwrap();

    let entry = entry_at(&temp_dir, "sub_directory").expect("entry reads");
    assert_eq!(entry.name, "sub_directory");
    assert!(entry.is_directory);
    assert!(entry.size.is_none());
}

#[test]
fn test_entry_from_lstat_is_none_for_a_vanished_entry() {
    let temp_dir = TestDir::new("process_vanished_test");
    assert!(entry_at(&temp_dir, "gone.txt").is_none());
}

// ============================================================================
// Tests for `is_hidden`: name, and (macOS) `UF_HIDDEN`
// ============================================================================

#[test]
fn test_entry_from_lstat_marks_a_dotfile_hidden() {
    let temp_dir = TestDir::new("dotfile_hidden_test");
    fs::write(temp_dir.join(".secret"), "x").unwrap();

    let entry = entry_at(&temp_dir, ".secret").expect("entry reads");
    assert!(entry.is_hidden, "a dotfile must be marked hidden");
}

#[test]
fn test_entry_from_lstat_leaves_an_ordinary_file_visible() {
    let temp_dir = TestDir::new("ordinary_not_hidden_test");
    fs::write(temp_dir.join("visible.txt"), "x").unwrap();

    let entry = entry_at(&temp_dir, "visible.txt").expect("entry reads");
    assert!(!entry.is_hidden, "an ordinary file must not be marked hidden");
}

/// `chflags hidden` sets `UF_HIDDEN` in `st_flags`, and macOS honors it with no
/// dot in the name at all. `LocalPosixVolume` must too.
#[cfg(target_os = "macos")]
#[test]
fn test_entry_from_lstat_honors_uf_hidden_with_no_dot_in_the_name() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let temp_dir = TestDir::new("uf_hidden_test");
    let file_path = temp_dir.join("no_dot_but_hidden.txt");
    fs::write(&file_path, "x").unwrap();

    let c_path = CString::new(file_path.as_os_str().as_bytes()).expect("path has no interior NUL");
    // SAFETY: `c_path` is a live, NUL-terminated C string naming a regular file
    // this test just created and owns for its whole body; `chflags` only writes
    // that file's on-disk flags, nothing is read back through the pointer.
    let result = unsafe { libc::chflags(c_path.as_ptr(), libc::UF_HIDDEN) };
    assert_eq!(
        result, 0,
        "chflags(UF_HIDDEN) must succeed on a file this test just created"
    );

    let entry = entry_at(&temp_dir, "no_dot_but_hidden.txt").expect("entry reads");
    assert!(
        entry.is_hidden,
        "UF_HIDDEN must mark an entry hidden even though its name carries no dot"
    );
}

// ============================================================================
// Tests for withhold_unclaimed_dates
// ============================================================================

/// A listing never carries a date its folder's claim denies: an ext4 mount
/// point inside an NFS folder has a birth time of its own, but the NFS folder's
/// listing claims no Created date.
#[test]
fn an_entry_loses_the_dates_its_folder_does_not_claim() {
    let entry = FileEntry {
        created_at: Some(1_600_000_000),
        added_at: Some(1_700_000_000),
        ..FileEntry::new("mnt".to_string(), "/nfs/mnt".to_string(), true, false)
    };

    let modified_only = withhold_unclaimed_dates(entry.clone(), SupportedDates::NONE);
    assert_eq!((modified_only.created_at, modified_only.added_at), (None, None));

    let created_only = SupportedDates {
        created: true,
        added: false,
    };
    let kept = withhold_unclaimed_dates(entry, created_only);
    assert_eq!((kept.created_at, kept.added_at), (Some(1_600_000_000), None));
}
