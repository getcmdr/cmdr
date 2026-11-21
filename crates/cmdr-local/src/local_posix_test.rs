//! `LocalPosixVolume`'s own cells: its `os_path` crossing and the bridge shapes it still
//! takes, Finder's `/.hidden`, POSIX symlinks and hardlinks, modes and the sealed system
//! volume, a host keyed by native paths, and the no-replace rename's degradation (errnos
//! injected). The cells every OS shares are in `local_volume_test.rs`.

#![cfg(unix)] // POSIX internals: `os_path`, symlinks, hardlinks, modes, errnos

use super::*;
use crate::test_support::at;
use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{MutationEvent, UnwritableReason, Volume, WatchCoverage, WriteAccess};
use std::path::Path;

// ── The `os_path` crossing, and the bridge shapes it takes ────────────

#[test]
fn test_resolve_empty_path_returns_root() {
    let volume = LocalPosixVolume::local_folder("Test", "/tmp", VolumeHost::detached());
    assert_eq!(
        volume.os_path(&AppPath::legacy_unchecked(Path::new(""))),
        Path::new("/tmp")
    );
}

#[test]
fn test_resolve_dot_returns_root() {
    let volume = LocalPosixVolume::local_folder("Test", "/tmp", VolumeHost::detached());
    assert_eq!(
        volume.os_path(&AppPath::legacy_unchecked(Path::new("."))),
        Path::new("/tmp")
    );
}

#[test]
fn test_resolve_relative_path_joins_with_root() {
    let volume = LocalPosixVolume::local_folder("Test", "/tmp", VolumeHost::detached());
    assert_eq!(
        volume.os_path(&AppPath::legacy_unchecked(Path::new("subdir/file.txt"))),
        Path::new("/tmp/subdir/file.txt")
    );
}

#[test]
fn test_resolve_absolute_path_treats_as_relative() {
    let volume = LocalPosixVolume::local_folder("Test", "/tmp", VolumeHost::detached());
    // Absolute paths should be treated as relative to volume root
    assert_eq!(
        volume.os_path(&AppPath::legacy_unchecked(Path::new("/subdir/file.txt"))),
        Path::new("/tmp/subdir/file.txt")
    );
}

#[tokio::test]
async fn test_exists_returns_true_for_root() {
    let volume = LocalPosixVolume::local_folder("Test", "/tmp", VolumeHost::detached());
    assert!(volume.exists(&AppPath::legacy_unchecked(Path::new(""))).await);
    assert!(volume.exists(&AppPath::legacy_unchecked(Path::new("."))).await);
}

/// Finder's legacy `/.hidden` file only means anything at a volume's root: the
/// same name one level down is an ordinary, visible entry. `list_directory`
/// decides "is this a root listing" by comparing the resolved path against
/// `self.root`, so this exercises that wiring end to end rather than
/// `reading::list_directory_core_with_tally`'s `is_volume_root` flag in
/// isolation.
#[tokio::test]
async fn test_dot_hidden_file_hides_a_name_at_the_volume_root_only() {
    let test_dir = TestDir::new("dot_hidden_root_test");
    std::fs::write(test_dir.join(".hidden"), "secret_dir\n").unwrap();
    std::fs::create_dir(test_dir.join("secret_dir")).unwrap();
    std::fs::create_dir(test_dir.join("sub")).unwrap();
    std::fs::create_dir(test_dir.join("sub/secret_dir")).unwrap();

    let volume = LocalPosixVolume::local_folder("Test", &*test_dir, VolumeHost::detached());

    let root_entries = volume.list_directory(&at(&volume, ""), None).await.unwrap();
    let root_secret = root_entries
        .iter()
        .find(|e| e.name == "secret_dir")
        .expect("secret_dir exists at the root");
    assert!(
        root_secret.is_hidden,
        "a name listed in the root's /.hidden must be hidden"
    );

    let sub_entries = volume.list_directory(&at(&volume, "sub"), None).await.unwrap();
    let sub_secret = sub_entries
        .iter()
        .find(|e| e.name == "secret_dir")
        .expect("the identically-named entry exists one level down");
    assert!(
        !sub_secret.is_hidden,
        "/.hidden only applies at a volume root, not a directory under it"
    );
}

// ============================================================================
// Symlink edge case tests
// ============================================================================

#[tokio::test]
async fn test_symlink_to_file_detected() {
    use cmdr_fs::testing::symlink;
    use std::fs;

    // Create a test file and symlink in /tmp
    let test_dir = TestDir::new("symlink_file_test");

    let target_file = test_dir.join("target.txt");
    let link_file = test_dir.join("link_to_file.txt");

    fs::write(&target_file, "content").unwrap();
    symlink(&target_file, &link_file).unwrap();

    let volume = LocalPosixVolume::local_folder("Test", test_dir.to_str().unwrap(), VolumeHost::detached());

    // The symlink should exist
    assert!(volume.exists(&at(&volume, "link_to_file.txt")).await);

    // Get metadata - should report is_symlink=true, is_directory=false
    let metadata = volume.get_metadata(&at(&volume, "link_to_file.txt")).await.unwrap();
    assert!(metadata.is_symlink);
    assert!(!metadata.is_directory);
    assert_eq!(metadata.name, "link_to_file.txt");
}

#[tokio::test]
async fn test_symlink_to_directory_detected() {
    use cmdr_fs::testing::symlink;
    use std::fs;

    let test_dir = TestDir::new("symlink_dir_test");

    let target_dir = test_dir.join("target_dir");
    let link_to_dir = test_dir.join("link_to_dir");

    fs::create_dir(&target_dir).unwrap();
    symlink(&target_dir, &link_to_dir).unwrap();

    let volume = LocalPosixVolume::local_folder("Test", test_dir.to_str().unwrap(), VolumeHost::detached());

    // Get metadata - should report is_symlink=true AND is_directory=true
    let metadata = volume.get_metadata(&at(&volume, "link_to_dir")).await.unwrap();
    assert!(metadata.is_symlink);
    assert!(metadata.is_directory); // Target is a directory
    assert_eq!(metadata.name, "link_to_dir");
}

#[tokio::test]
async fn test_broken_symlink_still_exists() {
    use cmdr_fs::testing::symlink;

    let test_dir = TestDir::new("broken_symlink_test");

    let broken_link = test_dir.join("broken_link.txt");
    symlink("/definitely_does_not_exist_12345", &broken_link).unwrap();

    let volume = LocalPosixVolume::local_folder("Test", test_dir.to_str().unwrap(), VolumeHost::detached());

    // The broken symlink itself exists
    assert!(volume.exists(&at(&volume, "broken_link.txt")).await);

    // Can get metadata for the broken symlink
    let metadata = volume.get_metadata(&at(&volume, "broken_link.txt")).await.unwrap();
    assert!(metadata.is_symlink);
    assert!(!metadata.is_directory); // Target doesn't exist, so defaults to false
}

// ============================================================================
// Copy operation tests
// ============================================================================

/// The blocking walk records every entry it counts, named under the paths the
/// caller passed (volume-relative here), never the resolved absolute ones.
#[tokio::test]
async fn a_batch_scan_records_every_entry_under_the_callers_paths() {
    use cmdr_fs::testing::scan_recording::{Recorded, ScanRecording};
    use cmdr_fs::volume::{ScanBoundary, ScanRecorder};
    use std::fs;
    use std::sync::Arc;

    let test_dir = TestDir::new("scan_records");
    fs::create_dir_all(test_dir.join("mydir/nested")).unwrap();
    fs::write(test_dir.join("mydir/a.txt"), "123").unwrap();
    fs::write(test_dir.join("mydir/nested/b.txt"), "A").unwrap();
    fs::write(test_dir.join("loose.txt"), "xy").unwrap();
    let volume = LocalPosixVolume::local_folder("Test", test_dir.to_str().unwrap(), VolumeHost::detached());
    let recording = Arc::new(ScanRecording::default());
    let boundary = ScanBoundary::silent().recording_to(Arc::clone(&recording) as Arc<dyn ScanRecorder>);

    let batch = volume
        .scan_for_copy_batch_with_boundary(
            &[
                AppPath::legacy_unchecked(&std::path::PathBuf::from("mydir")).into_owned(),
                AppPath::legacy_unchecked(&std::path::PathBuf::from("loose.txt")).into_owned(),
            ],
            &boundary,
        )
        .await
        .unwrap();

    assert_eq!(batch.aggregate.file_count, 3, "the totals don't change");
    assert_eq!(
        recording.sorted(),
        vec![
            Recorded::Dir("mydir".into()),
            Recorded::Dir("mydir/nested".into()),
            Recorded::File("loose.txt".into(), 2),
            Recorded::File("mydir/a.txt".into(), 3),
            Recorded::File("mydir/nested/b.txt".into(), 1),
        ]
    );
}

/// A tree where one 1 KiB inode is shared by three hardlinks plus one
/// standalone 4 KiB file. `total_bytes` is the cross-volume write footprint
/// (every link materializes separately at the destination): 3 * 1024 + 4096
/// = 7168. `dedup_bytes` is the source's on-disk footprint (`du`-equivalent,
/// each inode once): 1024 + 4096 = 5120. The Copy dialog shows the first as
/// the headline transfer size and the second as informational context.
#[tokio::test]
async fn test_scan_for_copy_dedupes_hardlinks_for_source_size_only() {
    use std::fs;

    let test_dir = TestDir::new("scan_copy_hardlink_test");

    let tree = test_dir.join("tree");
    fs::create_dir(&tree).unwrap();
    let original = tree.join("original");
    fs::write(&original, vec![0u8; 1024]).unwrap();
    fs::hard_link(&original, tree.join("hardlink_a")).unwrap();
    fs::hard_link(&original, tree.join("hardlink_b")).unwrap();
    fs::write(tree.join("standalone"), vec![0u8; 4096]).unwrap();

    let volume = LocalPosixVolume::local_folder("Test", test_dir.to_str().unwrap(), VolumeHost::detached());
    let result = volume.scan_for_copy(&at(&volume, "tree")).await.unwrap();

    assert_eq!(result.file_count, 4);
    assert_eq!(
        result.total_bytes, 7168,
        "write footprint counts every hardlink (none survive a cross-volume copy)"
    );
    assert_eq!(
        result.dedup_bytes, 5120,
        "source size counts each inode once, matching `du`"
    );
}

#[tokio::test]
async fn test_list_directory_includes_symlinks() {
    use cmdr_fs::testing::symlink;
    use std::fs;

    let test_dir = TestDir::new("symlink_list_test");

    // Create a regular file, a directory, and symlinks to each
    let file = test_dir.join("file.txt");
    let dir = test_dir.join("dir");
    let link_to_file = test_dir.join("link_to_file");
    let link_to_dir = test_dir.join("link_to_dir");

    fs::write(&file, "content").unwrap();
    fs::create_dir(&dir).unwrap();
    symlink(&file, &link_to_file).unwrap();
    symlink(&dir, &link_to_dir).unwrap();

    let volume = LocalPosixVolume::local_folder("Test", test_dir.to_str().unwrap(), VolumeHost::detached());
    let entries = volume.list_directory(&at(&volume, ""), None).await.unwrap();

    // Should have 4 entries
    assert_eq!(entries.len(), 4);

    // Find the symlinks
    let link_file_entry = entries.iter().find(|e| e.name == "link_to_file").unwrap();
    assert!(link_file_entry.is_symlink);
    assert!(!link_file_entry.is_directory);

    let link_dir_entry = entries.iter().find(|e| e.name == "link_to_dir").unwrap();
    assert!(link_dir_entry.is_symlink);
    assert!(link_dir_entry.is_directory); // Points to directory
}

/// What a local volume tells its host after a mutation: one stat-backed
/// `DirectoryChange` per mutation, for the folder it happened in.
#[tokio::test]
async fn a_local_mutation_reports_one_change_to_its_host() {
    use cmdr_fs::volume::DirectoryChange;
    use cmdr_fs::volume::host::listings::RecordingListings;
    use std::sync::Arc;

    let dir = TestDir::new("local_posix_notify_host");
    let listings = Arc::new(RecordingListings::new());
    let host = VolumeHost::builder().listings(listings.clone()).build();
    let volume = LocalPosixVolume::local_folder("Test", &*dir, host);

    std::fs::write(dir.join("landed.txt"), b"x").expect("writing the new file");
    volume
        .notify_mutation("vol", volume.root(), MutationEvent::Created("landed.txt".to_string()))
        .await;
    volume
        .notify_mutation("vol", volume.root(), MutationEvent::Deleted("gone.txt".to_string()))
        .await;

    let changes = listings.changes();
    assert_eq!(changes.len(), 2, "one change per mutation");
    assert!(
        matches!(&changes[0], (id, parent, DirectoryChange::Added(entry))
            if id == "vol" && parent == &*dir && entry.name == "landed.txt" && entry.size == Some(1)),
        "a create is reported with the entry it stat'ed"
    );
    assert!(
        matches!(&changes[1], (_, _, DirectoryChange::Removed(name)) if name == "gone.txt"),
        "a delete is reported by name"
    );
}

/// A local volume's watch coverage is its host's answer for the absolute path,
/// so a volume rooted below `/` still asks about the right directory.
#[test]
fn a_local_volume_asks_its_host_about_the_absolute_path() {
    use cmdr_fs::volume::host::listings::RecordingListings;
    use std::sync::Arc;

    let dir = TestDir::new("local_posix_coverage_host");
    let listings =
        RecordingListings::new().with_local_watch_coverage(dir.join("shown"), WatchCoverage::ThisMachineOnly);
    let host = VolumeHost::builder().listings(Arc::new(listings)).build();
    let volume = LocalPosixVolume::local_folder("Test", &*dir, host);

    assert_eq!(
        volume.listing_watch_coverage(&at(&volume, "shown")),
        WatchCoverage::ThisMachineOnly
    );
    assert_eq!(
        volume.listing_watch_coverage(&at(&volume, "elsewhere")),
        WatchCoverage::None
    );
}

/// A folder this user can write into is writable, and a folder a copy would
/// create answers for the nearest one that exists.
#[tokio::test]
async fn write_access_of_a_writable_folder_and_of_one_a_copy_would_create() {
    let dir = TestDir::new("local_posix_write_access");
    let volume = LocalPosixVolume::local_folder("Test", &*dir, VolumeHost::detached());
    assert_eq!(volume.write_access_at(app_path!("/")).await, WriteAccess::Writable);
    assert_eq!(
        volume.write_access_at(app_path!("/new/deeper")).await,
        WriteAccess::Writable
    );
}

/// A folder whose mode takes write permission away answers `NoPermission`, which
/// `access(W_OK)`'s errno tells apart from a read-only filesystem.
#[tokio::test]
async fn write_access_of_a_folder_without_write_permission_is_no_permission() {
    use std::os::unix::fs::PermissionsExt;

    // SAFETY: `geteuid` has no preconditions; it only reads this process's credentials.
    if unsafe { libc::geteuid() } == 0 {
        // Root writes past a folder's mode, so there's nothing to assert.
        return;
    }
    let dir = TestDir::new("local_posix_write_access_locked");
    let locked = dir.join("locked");
    std::fs::create_dir(&locked).expect("making the folder");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).expect("taking write away");
    let volume = LocalPosixVolume::local_folder("Test", &*dir, VolumeHost::detached());

    let access = volume.write_access_at(app_path!("/locked")).await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("giving write back for cleanup");

    assert_eq!(
        access,
        WriteAccess::Unwritable {
            reason: UnwritableReason::NoPermission
        }
    );
}

/// ❗ macOS mounts its sealed system volume read-only (macOS 11 and later), the
/// one read-only filesystem every Mac has to test against.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn write_access_on_the_sealed_system_volume_is_read_only() {
    let volume = LocalPosixVolume::local_folder("Macintosh HD", "/", VolumeHost::detached());
    assert_eq!(
        volume.write_access_at(app_path!("/System/Library")).await,
        WriteAccess::Unwritable {
            reason: UnwritableReason::ReadOnlyFilesystem
        }
    );
}

/// ❗ A cross-volume copy onto a read-only mount (in ERR-P7XKX, an app's
/// installer image) is refused at `File::create` with `EROFS`. The volume must
/// say `ReadOnly`, so the transfer names a read-only destination instead of a
/// generic failure whose Retry can only fail again.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_stream_onto_a_read_only_filesystem_is_read_only() {
    use cmdr_fs::volume::{InMemoryVolume, VolumeError, WriteMode};

    let source = InMemoryVolume::new("Source");
    source
        .create_file(app_path!("/shortcut.lnk"), b"lnk")
        .await
        .expect("seeding the source");
    let stream = source
        .open_read_stream(app_path!("/shortcut.lnk"))
        .await
        .expect("opening the source");
    let size = stream.total_size();
    let volume = LocalPosixVolume::local_folder("Macintosh HD", "/", VolumeHost::detached());

    let err = volume
        .write_from_stream(
            app_path!("/cmdr-read-only-probe.lnk"),
            WriteMode::CreateOrReplace,
            size,
            stream,
            &|_| std::ops::ControlFlow::Continue(()),
        )
        .await
        .expect_err("the sealed system volume takes no writes");

    assert!(matches!(err, VolumeError::ReadOnly(_)), "got {err:?}");
}

// ── Exclusive rename where the kernel has no atomic no-replace flag ──────────
//
// `renamex_np` / `renameat2` exist on APFS and ext4 and nowhere near every
// filesystem a Mac mounts. macOS's smbfs answers `ENOTSUP` (errno 45), so a
// primitive that takes the flag's failure at face value can't rename anything on
// a mounted SMB share. ERR-8RFN4 is a user who could rename a file through
// `/Volumes/…` under Macintosh HD and never through the same share's own volume
// entry, which is the route that reaches `LocalPosixVolume::rename`.
//
// The attempt is injected because no filesystem these tests can mount refuses
// the flag, and `testing::disk_images` is explicitly not for minting a FAT image.

/// An exclusive-rename attempt that always fails, so the degradation is
/// reachable on the APFS scratch dir a test really runs on.
fn always_failing_attempt(errno: i32) -> impl Fn(&Path, &Path) -> std::io::Result<()> {
    move |_, _| Err(std::io::Error::from_raw_os_error(errno))
}

#[test]
fn an_exclusive_rename_lands_on_a_filesystem_without_the_flag() {
    let test_dir = TestDir::new("rename_exclusive_unsupported");
    let source = test_dir.join("old.txt");
    let destination = test_dir.join("new.txt");
    std::fs::write(&source, b"content").expect("seeding the source");

    local_posix::rename_exclusive_with(&source, &destination, always_failing_attempt(libc::ENOTSUP))
        .expect("a filesystem without the flag still renames");

    assert!(!source.exists());
    assert_eq!(
        std::fs::read(&destination).expect("reading the destination"),
        b"content"
    );
}

/// ❗ The degradation is racy, never permissive: losing the flag must not turn a
/// rename into the silent clobber a plain POSIX `rename(2)` would perform.
#[test]
fn an_exclusive_rename_without_the_flag_still_refuses_an_occupied_destination() {
    let test_dir = TestDir::new("rename_exclusive_unsupported_occupied");
    let source = test_dir.join("source.txt");
    let destination = test_dir.join("target.txt");
    std::fs::write(&source, b"source").expect("seeding the source");
    std::fs::write(&destination, b"target").expect("seeding the destination");

    let err = local_posix::rename_exclusive_with(&source, &destination, always_failing_attempt(libc::ENOTSUP))
        .expect_err("an occupied destination is still a refusal");

    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&source).expect("reading the source"), b"source");
    assert_eq!(std::fs::read(&destination).expect("reading the destination"), b"target");
}

/// ❗ Only the flag's absence degrades. A refusal the filesystem meant lands on
/// the caller as itself, or the retry becomes a second, quieter way to fail.
#[test]
fn an_exclusive_rename_propagates_a_failure_that_is_not_about_the_flag() {
    let test_dir = TestDir::new("rename_exclusive_other_errno");
    let source = test_dir.join("source.txt");
    let destination = test_dir.join("target.txt");
    std::fs::write(&source, b"source").expect("seeding the source");

    let err = local_posix::rename_exclusive_with(&source, &destination, always_failing_attempt(libc::EACCES))
        .expect_err("a permission refusal is the filesystem's own answer");

    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(source.exists(), "nothing moved");
    assert!(!destination.exists());
}

// ── What a stat of the destination proves ───────────────────────────────────
//
// The degraded path asks the filesystem whether the name is taken, and the
// answer has THREE states. A stat that couldn't answer proves nothing, and the
// two tests below are the ones that stop it being read as "the name is free".
//
// A stat that fails while the rename beside it succeeds can't be staged on a
// real filesystem: both resolve the same path through the same permissions, so
// whatever stops one stops the other. Only a TRANSIENT failure (a share
// reconnecting, a marginal stick) separates them, which is why the verdict is
// handed in rather than provoked.

#[test]
fn a_stat_that_answered_tells_the_rename_which_way_to_go() {
    let test_dir = TestDir::new("name_state_answers");
    let occupied = test_dir.join("occupied.txt");
    std::fs::write(&occupied, b"here").expect("seeding the occupied name");

    assert!(matches!(
        local_posix::name_state(std::fs::symlink_metadata(&occupied)),
        local_posix::NameState::Occupied
    ));
    assert!(matches!(
        local_posix::name_state(std::fs::symlink_metadata(test_dir.join("nothing.txt"))),
        local_posix::NameState::Free
    ));
}

/// ❗ The clobber this whole primitive exists to prevent. A stat that came back
/// with anything other than "it isn't there" has told us nothing about the
/// name, so the rename must refuse rather than assume it's free and destroy
/// whatever is actually sitting there.
#[test]
fn a_destination_we_could_not_stat_is_never_treated_as_a_free_name() {
    let test_dir = TestDir::new("name_state_unknown");
    let source = test_dir.join("source.txt");
    let destination = test_dir.join("target.txt");
    std::fs::write(&source, b"source").expect("seeding the source");
    std::fs::write(&destination, b"the user's file").expect("seeding the destination");

    let unreadable = local_posix::NameState::Unknown(std::io::Error::from_raw_os_error(libc::EIO));
    let err = local_posix::rename_if_free(&source, &destination, unreadable)
        .expect_err("a stat that couldn't answer is never a free name");

    assert_eq!(
        err.raw_os_error(),
        Some(libc::EIO),
        "the filesystem's own reason rides out"
    );
    assert_eq!(
        std::fs::read(&destination).expect("reading the destination"),
        b"the user's file",
        "the file that was there survived"
    );
    assert!(source.exists(), "nothing moved");
}
