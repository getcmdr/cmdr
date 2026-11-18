//! The shared `Volume` conformance promises, asserted against `LocalWindowsVolume` on real
//! NTFS (the Windows builder, plan § "Windows build state").
//!
//! The cell list mirrors `local_posix_conformance_test.rs`. A promise the Windows volume
//! doesn't keep yet is `#[ignore]`d with the piece of task 35 that owns it, never left out
//! and never passing by a default: an ignored cell is a named debt, a missing one is a
//! silent pass.

#![cfg(target_os = "windows")]

use std::path::{Path, PathBuf};

use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::Volume;
use cmdr_fs::volume::conformance;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::path_grammar::{self, PathFlavor};

use super::*;

/// The app path of a test folder the OS handed back as `C:\…`.
pub(crate) fn app_root(dir: &Path) -> PathBuf {
    let native = dir.to_str().expect("the test folder's path is Unicode");
    PathBuf::from(
        path_grammar::native_to_app(native, PathFlavor::Windows).expect("the test folder is an absolute Windows path"),
    )
}

/// A volume over the test folder, the way the app registers one: rooted at an app path.
pub(crate) fn volume_at(dir: &TestDir) -> LocalWindowsVolume {
    LocalWindowsVolume::local_folder("Test", app_root(dir), VolumeHost::detached())
}

#[tokio::test]
async fn delete_honors_the_shared_non_recursion_contract() {
    let test_dir = TestDir::new("win_delete_non_recursion");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("album")).await.expect("mkdir");
    volume
        .create_file(Path::new("album/keep.txt"), b"content")
        .await
        .expect("create");

    conformance::assert_delete_leaves_a_non_empty_dir_intact(&volume, Path::new("album"), "keep.txt").await;
}

#[tokio::test]
async fn delete_files_honors_the_shared_batch_contract() {
    let test_dir = TestDir::new("win_delete_files_batch");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("level")).await.expect("mkdir");
    for name in ["a.txt", "b.txt", "kept.txt"] {
        volume
            .create_file(&Path::new("level").join(name), b"bytes")
            .await
            .expect("create");
    }

    conformance::assert_delete_files_removes_exactly_what_it_names(
        &volume,
        [Path::new("level/a.txt"), Path::new("level/b.txt")],
        Path::new("level/kept.txt"),
    )
    .await;
}

/// `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`: the filesystem refuses the taken name in
/// the same step that renames.
#[tokio::test]
async fn rename_honors_the_shared_no_clobber_contract() {
    let test_dir = TestDir::new("win_rename_no_clobber");
    let volume = volume_at(&test_dir);
    volume
        .create_file(Path::new("source.txt"), b"source")
        .await
        .expect("create");
    volume
        .create_file(Path::new("target.txt"), b"the user's target file")
        .await
        .expect("create");

    conformance::assert_rename_refuses_an_existing_destination(
        &volume,
        Path::new("source.txt"),
        Path::new("target.txt"),
    )
    .await;
}

#[tokio::test]
async fn forced_rename_honors_the_shared_non_empty_folder_contract() {
    let test_dir = TestDir::new("win_forced_rename_non_empty");
    let volume = volume_at(&test_dir);
    volume
        .create_file(Path::new("source.txt"), b"source")
        .await
        .expect("create");
    volume.create_directory(Path::new("source-dir")).await.expect("mkdir");
    volume.create_directory(Path::new("album")).await.expect("mkdir");
    volume
        .create_file(Path::new("album/source.txt"), b"the user's photo")
        .await
        .expect("create");

    for from in ["source.txt", "source-dir"] {
        conformance::assert_forced_rename_keeps_a_non_empty_dir(&volume, Path::new(from), Path::new("album")).await;
    }
}

#[tokio::test]
async fn create_file_honors_the_shared_no_clobber_contract() {
    let test_dir = TestDir::new("win_create_file_no_clobber");
    let volume = volume_at(&test_dir);
    volume
        .create_file(Path::new("notes.txt"), b"the user's notes")
        .await
        .expect("create");

    conformance::assert_create_file_refuses_to_clobber(&volume, Path::new("notes.txt"), b"new").await;
}

#[tokio::test]
async fn listing_dates_honor_the_shared_claim_contract() {
    let test_dir = TestDir::new("win_listed_dates");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("dated")).await.expect("mkdir");
    volume.create_directory(Path::new("dated/folder")).await.expect("mkdir");
    volume
        .create_file(Path::new("dated/file.txt"), b"bytes")
        .await
        .expect("create");

    conformance::assert_listing_dates_are_claimed(&volume, Path::new("dated")).await;
    let claimed = volume.supported_dates(Path::new("dated")).await;
    assert!(claimed.created, "NTFS records a birth time");
    assert!(!claimed.added, "Windows records no added date");
}

#[tokio::test]
async fn create_directory_all_honors_the_shared_honesty_contract() {
    let test_dir = TestDir::new("win_mkdir_all_honesty");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("album")).await.expect("mkdir");

    conformance::assert_create_directory_all_reports_an_existing_dir_honestly(&volume, Path::new("album")).await;
}

#[tokio::test]
async fn create_directory_all_honors_the_shared_file_in_the_way_contract() {
    let test_dir = TestDir::new("win_mkdir_all_file_in_the_way");
    let volume = volume_at(&test_dir);
    volume
        .create_file(Path::new("notes"), b"the user's notes")
        .await
        .expect("create");

    conformance::assert_create_directory_all_refuses_a_file_in_the_way(&volume, Path::new("notes")).await;
}

#[tokio::test]
async fn is_writable_honors_the_shared_declaration_contract() {
    let test_dir = TestDir::new("win_is_writable");
    let volume = volume_at(&test_dir);

    conformance::assert_writability_matches_the_mutations_offered(&volume, Path::new("scratch")).await;
}

#[tokio::test]
async fn copy_within_honors_the_shared_handshake_contract() {
    let test_dir = TestDir::new("win_copy_within");
    let volume = volume_at(&test_dir);
    volume
        .create_file(Path::new("original.txt"), b"bytes")
        .await
        .expect("create");

    conformance::assert_copy_within_matches_what_is_offered(
        &volume,
        Path::new("original.txt"),
        Path::new("duplicate.txt"),
    )
    .await;
}

#[tokio::test]
async fn not_found_honors_the_shared_path_payload_contract() {
    let test_dir = TestDir::new("win_not_found_payload");
    let volume = volume_at(&test_dir);

    conformance::assert_not_found_carries_the_path(&volume, Path::new("no-such-file.txt")).await;
}

#[tokio::test]
async fn conflict_scan_honors_the_shared_missing_destination_contract() {
    let test_dir = TestDir::new("win_conflict_scan_missing");
    let volume = volume_at(&test_dir);

    conformance::assert_conflict_scan_reads_a_missing_destination_as_empty(&volume, Path::new("not-created-yet")).await;
}

// ── Not kept yet: each names the piece of task 35 that owns it ─────────────────────

#[tokio::test]
#[ignore = "task 35, streams: LocalWindowsVolume has no write_from_stream yet"]
async fn write_from_stream_create_new_honors_the_shared_no_clobber_contract() {
    let test_dir = TestDir::new("win_write_create_new");
    let volume = volume_at(&test_dir);
    volume
        .create_file(Path::new("notes.txt"), b"the user's notes")
        .await
        .expect("create");

    conformance::assert_write_from_stream_create_new_refuses_to_clobber(
        &volume,
        Path::new("notes.txt"),
        Path::new("fresh.txt"),
        b"new",
    )
    .await;
}

#[tokio::test]
#[ignore = "task 35, streams: LocalWindowsVolume has no read or write stream yet"]
async fn a_copy_keeps_the_source_date_per_the_shared_contract() {
    let test_dir = TestDir::new("win_dated_write");
    let volume = volume_at(&test_dir);

    conformance::assert_write_from_stream_keeps_the_source_date(
        &volume,
        Path::new("dated.txt"),
        std::time::Duration::ZERO,
    )
    .await;
    conformance::assert_read_stream_reports_the_listed_date(&volume, Path::new("dated.txt")).await;
    conformance::assert_landed_time_agrees_with_get_metadata(&volume, Path::new("landed.txt")).await;
}

#[tokio::test]
#[ignore = "task 35, metadata: LocalWindowsVolume has no apply_metadata yet"]
async fn apply_metadata_honors_the_shared_folder_date_contract() {
    let test_dir = TestDir::new("win_folder_date");
    let volume = volume_at(&test_dir);

    conformance::assert_apply_metadata_dates_a_folder(&volume, Path::new("dated"), std::time::Duration::ZERO).await;
}

#[tokio::test]
#[ignore = "task 35, metadata and streams: no metadata caps, apply_metadata, or streams yet"]
async fn local_disk_honors_the_shared_metadata_contract() {
    let test_dir = TestDir::new("win_metadata");
    let volume = volume_at(&test_dir);

    conformance::assert_write_from_stream_keeps_metadata(&volume, Path::new("born.txt")).await;
    conformance::assert_read_stream_reports_the_listed_metadata(&volume, Path::new("born.txt")).await;
    conformance::assert_apply_metadata_keeps(&volume, Path::new("born")).await;
}

#[tokio::test]
#[ignore = "task 35, streams: LocalWindowsVolume has no read stream yet, so no export"]
async fn export_honors_the_shared_handshake_contract() {
    let test_dir = TestDir::new("win_export_handshake");
    let volume = volume_at(&test_dir);
    let content = b"the bytes a copy would move";
    volume
        .create_file(Path::new("exported.txt"), content)
        .await
        .expect("create");

    conformance::assert_export_matches_the_bytes_offered(&volume, Path::new("exported.txt"), content).await;
}

/// A junction to `target` at `link`, both inside `dir`. Junctions need no privilege (a
/// symlink needs Developer Mode or an elevated token), so these cells build the link the
/// volume can't make yet itself (`create_symlink`, task 35) with `mklink /J`.
pub(crate) fn junction(dir: &Path, link: &str, target: &str) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dir.join(link))
        .arg(dir.join(target))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("run mklink");
    assert!(status.success(), "mklink /J {link} {target}");
}

#[tokio::test]
async fn create_directory_all_honors_the_shared_through_a_link_contract() {
    let test_dir = TestDir::new("win_mkdir_all_through_a_link");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("real")).await.expect("mkdir");
    junction(&test_dir, "link", "real");

    conformance::assert_create_directory_all_goes_through_a_link_to_a_folder(
        &volume,
        Path::new("link"),
        Path::new("real"),
    )
    .await;
}

#[tokio::test]
async fn delete_of_a_link_to_a_folder_honors_the_shared_contract() {
    let test_dir = TestDir::new("win_delete_of_a_link");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("real")).await.expect("mkdir");
    volume
        .create_file(Path::new("real/keep.txt"), b"keep")
        .await
        .expect("create");
    junction(&test_dir, "link", "real");

    conformance::assert_delete_of_a_link_to_a_folder_removes_the_link_alone(
        &volume,
        Path::new("link"),
        Path::new("real"),
        "keep.txt",
    )
    .await;
}

#[tokio::test]
async fn a_link_to_a_folder_moves_as_the_link_per_the_shared_contract() {
    let test_dir = TestDir::new("win_link_move");
    let volume = volume_at(&test_dir);
    volume.create_directory(Path::new("real")).await.expect("mkdir");
    volume
        .create_file(Path::new("real/keep.txt"), b"keep")
        .await
        .expect("create");
    volume.create_directory(Path::new("moved")).await.expect("mkdir");
    junction(&test_dir, "link", "real");

    conformance::assert_a_link_to_a_folder_moves_as_the_link(
        &volume,
        Path::new("link"),
        Path::new("moved/link"),
        Path::new("real"),
        "keep.txt",
    )
    .await;
}

#[tokio::test]
#[ignore = "task 35, symlinks and junctions: can_hold_links is false until file vs dir symlinks land"]
async fn a_link_lands_verbatim() {
    let dir = TestDir::new("win_link_landing");
    let volume = volume_at(&dir);
    conformance::assert_a_link_lands_verbatim(&volume, Path::new("/")).await;
}

#[tokio::test]
#[ignore = "task 35, ADS-backed xattrs: supports_xattrs is false until alternate data streams back them"]
async fn xattrs_keep_the_shared_contract() {
    let test_dir = TestDir::new("win_xattrs_contract");
    let volume = volume_at(&test_dir);
    volume.create_file(Path::new("plain.txt"), b"x").await.expect("create");
    conformance::assert_xattrs_keep_their_promises(&volume, Path::new("plain.txt"), Path::new("gone.txt")).await;
}
