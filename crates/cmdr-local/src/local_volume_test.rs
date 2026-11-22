//! This OS's `LocalVolume` on real scratch folders, the cells every OS shares: what it
//! declares, the write ops and their refusals, listing and stat'ing, the conflict scan, space,
//! and listing progress. A cell the Windows volume can't run yet is ignored there with what it
//! lacks, a named debt. `LocalPosixVolume`'s own cells are in `local_posix_test.rs`, the
//! Windows sibling's in `local_windows_test.rs`.

use crate::LocalVolume;
use crate::test_support::{at, volume_at};
use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::AppTestDir;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{
    ListingProgress, MountClass, SourceItemInfo, SpaceInfo, StreamLength, Volume, VolumeError, WriteMode,
};
use std::path::Path;

/// The app path of a folder that's never there: a name under a fresh scratch folder.
fn missing_under(dir: &AppTestDir) -> AppPathBuf {
    dir.app_path()
        .join_relative(cmdr_fs::app_path::RelativeAppPath::new("definitely_does_not_exist_12345").unwrap())
        .unwrap()
}

#[test]
fn a_local_volume_keeps_its_name_and_root() {
    let dir = AppTestDir::new("name_and_root");
    let volume = LocalVolume::local_folder_at("Test Volume", dir.app_path().to_owned(), VolumeHost::detached());
    assert_eq!(volume.name(), "Test Volume");
    assert_eq!(volume.root(), dir.app_path());
}

// ── Whether a drive index can serve it, by the mount it sits on ───────

/// A volume over a scratch folder on `mount`; what it declares doesn't touch the disk.
fn on_mount(dir: &AppTestDir, mount: MountClass) -> LocalVolume {
    LocalVolume::on_mount_at("v", dir.app_path().to_owned(), mount, VolumeHost::detached())
}

#[test]
fn a_folder_on_a_local_disk_can_be_indexed() {
    let dir = AppTestDir::new("indexable_local");
    assert!(volume_at(&dir).capabilities().can_be_indexed);
    assert!(on_mount(&dir, MountClass::LocalDisk).capabilities().can_be_indexed);
}

#[test]
fn an_os_mounted_smb_share_can_be_indexed_because_the_gate_upgrades_it() {
    let dir = AppTestDir::new("indexable_smb");
    assert!(on_mount(&dir, MountClass::SmbShare).capabilities().can_be_indexed);
}

/// Regression anchor (follow-up to `ERR-JUCNB`): an NFS, AFP, Finder-WebDAV, or FUSE mount was
/// offered "Turn on indexing", and the enable could only be refused, because the capability
/// answered per backend while the router answered per mount. The capability is the answer the
/// switcher, the first-connect prompt, search, and `start_volume`'s door all read.
#[test]
fn a_mount_on_any_other_network_filesystem_cant_be_indexed() {
    let dir = AppTestDir::new("indexable_other_network");
    assert!(!on_mount(&dir, MountClass::OtherNetwork).capabilities().can_be_indexed);
}

/// Trash goes through the OS on raw paths, and the OS keeps a Trash only on a local disk: on a
/// network mount it refuses every item and deletes nothing (#747).
#[test]
fn only_a_local_disk_keeps_a_trash() {
    let dir = AppTestDir::new("trash_by_mount");
    assert!(volume_at(&dir).has_trash());
    assert!(on_mount(&dir, MountClass::LocalDisk).has_trash());
    assert!(!on_mount(&dir, MountClass::SmbShare).has_trash());
    assert!(!on_mount(&dir, MountClass::OtherNetwork).has_trash());
}

#[test]
fn a_rerooted_volume_keeps_its_mount_class() {
    let dir = AppTestDir::new("reroot_mount_class");
    let moved = on_mount(&dir, MountClass::OtherNetwork)
        .rerooted(&at(&volume_at(&dir), "export-1"))
        .expect("a local volume re-roots");
    assert!(!moved.capabilities().can_be_indexed);
}

#[test]
fn a_local_volume_watches_its_listings() {
    let dir = AppTestDir::new("can_watch");
    assert!(volume_at(&dir).can_watch_listings());
}

#[test]
fn a_local_volume_streams_and_exports() {
    // Cross-volume copies pipe through `open_read_stream` and `write_from_stream`.
    let dir = AppTestDir::new("streams_and_exports");
    let volume = volume_at(&dir);
    assert!(volume.supports_streaming());
    assert!(volume.supports_export());
}

// ── Listing and stat'ing ──────────────────────────────────────────────

#[tokio::test]
async fn the_root_exists_and_a_missing_name_does_not() {
    let dir = AppTestDir::new("exists");
    let volume = volume_at(&dir);
    assert!(volume.exists(&at(&volume, "")).await);
    assert!(!volume.exists(&at(&volume, "definitely_does_not_exist_12345")).await);
}

#[tokio::test]
async fn the_root_lists_and_stats_as_a_folder() {
    let dir = AppTestDir::new("root_lists");
    std::fs::write(dir.native().join("a.txt"), b"x").unwrap();
    let volume = volume_at(&dir);

    let listed = volume.list_directory(&at(&volume, ""), None).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(volume.get_metadata(&at(&volume, "")).await.unwrap().is_directory);
}

#[tokio::test]
async fn a_missing_root_lists_as_an_error() {
    let dir = AppTestDir::new("missing_root");
    let volume = LocalVolume::local_folder_at("Test", missing_under(&dir), VolumeHost::detached());
    match volume.list_directory(&at(&volume, ""), None).await {
        Err(VolumeError::NotFound(_) | VolumeError::IoError { .. }) => (),
        other => panic!("expected NotFound or IoError, got: {other:?}"),
    }
}

#[tokio::test]
async fn get_metadata_of_a_missing_name_is_an_error() {
    let dir = AppTestDir::new("metadata_missing");
    let volume = volume_at(&dir);
    assert!(
        volume
            .get_metadata(&at(&volume, "definitely_does_not_exist_12345"))
            .await
            .is_err()
    );
}

// ── Write ops ─────────────────────────────────────────────────────────

#[tokio::test]
async fn create_delete_and_rename_land_on_disk() {
    use std::fs;

    let test_dir = AppTestDir::new("write_ops_test");
    let native = test_dir.native();
    let volume = volume_at(&test_dir);

    volume
        .create_file(&at(&volume, "test.txt"), b"hello world")
        .await
        .unwrap();
    assert_eq!(fs::read_to_string(native.join("test.txt")).unwrap(), "hello world");

    volume.create_directory(&at(&volume, "subdir")).await.unwrap();
    assert!(native.join("subdir").is_dir());

    volume.delete(&at(&volume, "test.txt")).await.unwrap();
    assert!(!native.join("test.txt").exists());

    volume.delete(&at(&volume, "subdir")).await.unwrap();
    assert!(!native.join("subdir").exists());

    // A rename with no force and no conflict.
    volume.create_file(&at(&volume, "old.txt"), b"content").await.unwrap();
    volume
        .rename(&at(&volume, "old.txt"), &at(&volume, "new.txt"), false)
        .await
        .unwrap();
    assert!(!native.join("old.txt").exists());
    assert!(native.join("new.txt").exists());
}

#[tokio::test]
async fn an_exclusive_rename_onto_a_taken_name_refuses_and_keeps_both_files() {
    let test_dir = AppTestDir::new("rename_taken");
    let volume = volume_at(&test_dir);
    volume.create_file(&at(&volume, "a.txt"), b"source").await.unwrap();
    volume
        .create_file(&at(&volume, "b.txt"), b"the user's file")
        .await
        .unwrap();

    let err = volume
        .rename(&at(&volume, "a.txt"), &at(&volume, "b.txt"), false)
        .await
        .expect_err("an exclusive rename onto a taken name must refuse");

    assert!(matches!(err, VolumeError::AlreadyExists(_)), "got {err:?}");
    assert_eq!(
        std::fs::read(test_dir.native().join("b.txt")).unwrap(),
        b"the user's file"
    );
    assert_eq!(std::fs::read(test_dir.native().join("a.txt")).unwrap(), b"source");
}

#[tokio::test]
async fn an_exclusive_rename_of_a_folder_onto_a_taken_name_refuses() {
    let test_dir = AppTestDir::new("rename_dir_taken");
    let volume = volume_at(&test_dir);
    volume.create_directory(&at(&volume, "from")).await.unwrap();
    volume.create_directory(&at(&volume, "to")).await.unwrap();
    volume.create_file(&at(&volume, "to/keep.txt"), b"keep").await.unwrap();

    let err = volume
        .rename(&at(&volume, "from"), &at(&volume, "to"), false)
        .await
        .expect_err("a folder onto a taken name must refuse");

    assert!(matches!(err, VolumeError::AlreadyExists(_)), "got {err:?}");
    assert_eq!(std::fs::read(test_dir.native().join("to/keep.txt")).unwrap(), b"keep");
    assert!(test_dir.native().join("from").is_dir());
}

#[tokio::test]
async fn a_forced_rename_overwrites_the_destination() {
    let test_dir = AppTestDir::new("rename_force_test");
    let volume = volume_at(&test_dir);
    volume
        .create_file(&at(&volume, "source.txt"), b"new content")
        .await
        .unwrap();
    volume
        .create_file(&at(&volume, "target.txt"), b"old content")
        .await
        .unwrap();

    volume
        .rename(&at(&volume, "source.txt"), &at(&volume, "target.txt"), true)
        .await
        .unwrap();
    assert!(!test_dir.native().join("source.txt").exists());
    assert_eq!(
        std::fs::read_to_string(test_dir.native().join("target.txt")).unwrap(),
        "new content"
    );
}

/// Regression for the high-severity audit finding: `create_file` is a no-overwrite contract;
/// the IPC layer maps `AlreadyExists` to a friendly "file already exists" error. Switching to
/// a truncating write here loses the user's data when the listing-cache pre-check race fails.
#[tokio::test]
async fn create_file_does_not_clobber_an_existing_file() {
    let test_dir = AppTestDir::new("create_file_no_clobber_test");
    let volume = volume_at(&test_dir);
    let target = test_dir.native().join("notes.txt");
    std::fs::write(&target, "important user data").unwrap();

    let result = volume.create_file(&at(&volume, "notes.txt"), b"").await;

    assert!(
        matches!(result, Err(VolumeError::AlreadyExists(_))),
        "expected AlreadyExists, got {result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "important user data",
        "original file contents must survive a colliding create_file"
    );
}

/// The node a folder is survives a rename (what a trip into the Trash is), and a new folder
/// under the freed name is a different one: the check a trash undo makes.
#[tokio::test]
#[cfg_attr(
    target_os = "windows",
    ignore = "LocalWindowsVolume has no node identity yet (the Recycle Bin undo's check)"
)]
async fn node_identity_follows_the_folder_not_its_name() {
    let test_dir = AppTestDir::new("node_identity_test");
    let volume = volume_at(&test_dir);
    volume.create_directory(&at(&volume, "Invoices")).await.unwrap();
    let original = volume.node_identity(&at(&volume, "Invoices")).await.unwrap();
    assert!(original.is_some(), "APFS and ext4 keep a birth time");

    volume
        .rename(&at(&volume, "Invoices"), &at(&volume, "Trashed"), false)
        .await
        .unwrap();
    assert_eq!(volume.node_identity(&at(&volume, "Trashed")).await.unwrap(), original);

    volume.create_directory(&at(&volume, "Invoices")).await.unwrap();
    assert_ne!(volume.node_identity(&at(&volume, "Invoices")).await.unwrap(), original);
    assert!(matches!(
        volume.node_identity(&at(&volume, "missing")).await,
        Err(VolumeError::NotFound(_))
    ));
}

// ── Copy: the scan and the streams ────────────────────────────────────

#[tokio::test]
#[cfg_attr(
    target_os = "windows",
    ignore = "task 35, copy scan: LocalWindowsVolume keeps the trait's default"
)]
async fn a_copy_scan_of_one_file_counts_its_bytes() {
    let test_dir = AppTestDir::new("scan_copy_file_test");
    std::fs::write(test_dir.native().join("test.txt"), "Hello, World!").unwrap();
    let volume = volume_at(&test_dir);

    let result = volume.scan_for_copy(&at(&volume, "test.txt")).await.unwrap();

    assert_eq!(result.file_count, 1);
    assert_eq!(result.dir_count, 0);
    assert_eq!(result.total_bytes, 13); // "Hello, World!" is 13 bytes
}

#[tokio::test]
#[cfg_attr(
    target_os = "windows",
    ignore = "task 35, copy scan: LocalWindowsVolume keeps the trait's default"
)]
async fn a_copy_scan_of_a_folder_counts_its_tree() {
    use std::fs;

    let test_dir = AppTestDir::new("scan_copy_dir_test");
    let subdir = test_dir.native().join("mydir");
    fs::create_dir(&subdir).unwrap();
    fs::write(subdir.join("file1.txt"), "123").unwrap();
    fs::write(subdir.join("file2.txt"), "456789").unwrap();
    fs::create_dir(subdir.join("nested")).unwrap();
    fs::write(subdir.join("nested/file3.txt"), "A").unwrap();
    let volume = volume_at(&test_dir);

    let result = volume.scan_for_copy(&at(&volume, "mydir")).await.unwrap();

    assert_eq!(result.file_count, 3);
    assert_eq!(result.dir_count, 1); // Just the nested dir (root not counted)
    assert_eq!(result.total_bytes, 10); // 3 + 6 + 1
    // No hardlinks: dedup'd source size equals the write footprint.
    assert_eq!(result.dedup_bytes, 10);
}

/// The shared stop assertions, over the backend that serves every OS-mounted network share.
///
/// ❗ This is the `/Volumes/<share>` case, and it is the one that hurts most: a `readdir` on a
/// NAS that has spun its disks down takes seconds and a subtree takes minutes, all of it inside
/// one `spawn_blocking`. The boundary here is per entry, through the blocking twin that parks
/// the pool thread.
#[tokio::test]
#[cfg_attr(
    target_os = "windows",
    ignore = "task 35, copy scan: LocalWindowsVolume keeps the trait's default"
)]
async fn a_batch_scan_stops_when_it_is_told_to() {
    let test_dir = AppTestDir::new("scan_stop_told");
    let tree = test_dir.native().join("tree");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(tree.join("a.txt"), "a").unwrap();
    std::fs::write(tree.join("b.txt"), "bb").unwrap();
    let volume = volume_at(&test_dir);

    cmdr_fs::volume::conformance::assert_batch_scan_stops_when_told(&volume, Path::new("tree")).await;
}

#[tokio::test]
#[cfg_attr(
    target_os = "windows",
    ignore = "task 35, copy scan: LocalWindowsVolume keeps the trait's default"
)]
async fn a_batch_scan_asks_its_boundary_inside_the_walk() {
    let test_dir = AppTestDir::new("scan_stop_per_entry");
    let tree = test_dir.native().join("tree");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(tree.join("a.txt"), "a").unwrap();
    std::fs::write(tree.join("b.txt"), "bb").unwrap();
    std::fs::create_dir(tree.join("nested")).unwrap();
    std::fs::write(tree.join("nested/c.txt"), "ccc").unwrap();
    let volume = volume_at(&test_dir);

    cmdr_fs::volume::conformance::assert_batch_scan_asks_inside_the_walk(&volume, Path::new("tree"), 4).await;
}

#[tokio::test]
async fn a_read_stream_hands_out_the_files_bytes() {
    let test_dir = AppTestDir::new("read_stream_src_test");
    std::fs::write(test_dir.native().join("source.txt"), "Test content").unwrap();
    let volume = volume_at(&test_dir);

    let mut stream = volume.open_read_stream(&at(&volume, "source.txt")).await.unwrap();
    assert_eq!(stream.total_size(), StreamLength::Known(12));
    let mut content = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        content.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(content, b"Test content");
}

/// A destination that stores modification times (S3's `x-amz-meta-mtime`) reads the source's
/// off the stream, so the local stream has to carry the file's own, to the nanosecond the
/// filesystem kept.
#[tokio::test]
async fn a_read_stream_reports_the_files_mtime() {
    let test_dir = AppTestDir::new("read_stream_mtime_test");
    let path = test_dir.native().join("dated.txt");
    std::fs::write(&path, "dated").unwrap();
    let mtime = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::new(1_354_040_105, 123_456_000);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    let volume = volume_at(&test_dir);

    let stream = volume.open_read_stream(&at(&volume, "dated.txt")).await.unwrap();
    assert_eq!(stream.modified_at(), Some(mtime));
}

#[tokio::test]
async fn a_read_stream_of_a_folder_is_refused() {
    let test_dir = AppTestDir::new("read_stream_dir_test");
    std::fs::create_dir(test_dir.native().join("sourcedir")).unwrap();
    let volume = volume_at(&test_dir);

    let result = volume.open_read_stream(&at(&volume, "sourcedir")).await;
    assert!(result.is_err(), "streaming a directory should fail");
}

#[tokio::test]
async fn a_write_from_stream_creates_the_file() {
    use cmdr_fs::app_path;
    use cmdr_fs::volume::InMemoryVolume;

    let test_dir = AppTestDir::new("write_from_stream_test");
    let source = InMemoryVolume::new("Source");
    source
        .create_file(app_path!("/local.txt"), b"Imported content")
        .await
        .unwrap();
    let stream = source.open_read_stream(app_path!("/local.txt")).await.unwrap();
    let size = stream.total_size();
    let volume = volume_at(&test_dir);

    let bytes = volume
        .write_from_stream(
            &at(&volume, "imported.txt"),
            WriteMode::CreateOrReplace,
            size,
            stream,
            &|_| std::ops::ControlFlow::Continue(()),
        )
        .await
        .unwrap();

    assert_eq!(bytes.bytes, 16); // "Imported content" is 16 bytes
    assert_eq!(
        std::fs::read_to_string(test_dir.native().join("imported.txt")).unwrap(),
        "Imported content"
    );
}

// ── The conflict scan ─────────────────────────────────────────────────

fn item(name: &str, size: u64, modified: Option<i64>) -> SourceItemInfo {
    SourceItemInfo {
        name: name.to_string(),
        size,
        modified,
        is_directory: false,
    }
}

#[tokio::test]
async fn a_conflict_scan_into_an_empty_folder_finds_none() {
    let test_dir = AppTestDir::new("conflicts_none_test");
    let volume = volume_at(&test_dir);

    let source_items = [item("newfile.txt", 100, None), item("another.txt", 200, None)];
    let conflicts = volume
        .scan_for_conflicts(&source_items, &at(&volume, ""))
        .await
        .unwrap();
    assert!(conflicts.is_empty());
}

#[tokio::test]
async fn a_conflict_scan_names_each_taken_name_with_both_sides() {
    let test_dir = AppTestDir::new("conflicts_some_test");
    std::fs::write(test_dir.native().join("existing.txt"), "Old content").unwrap();
    std::fs::write(test_dir.native().join("another.txt"), "Another old").unwrap();
    let volume = volume_at(&test_dir);

    let source_items = [
        item("existing.txt", 100, Some(1_700_000_000)),
        item("newfile.txt", 200, None),
        item("another.txt", 300, Some(1_700_000_000)),
    ];
    let conflicts = volume
        .scan_for_conflicts(&source_items, &at(&volume, ""))
        .await
        .unwrap();
    assert_eq!(conflicts.len(), 2);

    let existing_conflict = conflicts.iter().find(|c| c.source_path == "existing.txt").unwrap();
    assert_eq!(existing_conflict.source_size, 100);
    assert_eq!(existing_conflict.dest_size, 11); // "Old content" is 11 bytes
    assert_eq!(existing_conflict.source_modified, Some(1_700_000_000));
}

// ── Space and listing progress ────────────────────────────────────────

#[tokio::test]
async fn the_root_reports_a_bounded_capacity() {
    let test_dir = AppTestDir::new("space");
    let volume = volume_at(&test_dir);

    let space = volume.get_space_info().await.unwrap();

    // A mounted filesystem always has a ceiling, so the reading is always `Bounded`.
    let SpaceInfo::Bounded {
        total_bytes,
        available_bytes,
        used_bytes,
    } = space
    else {
        panic!("a local filesystem always has a capacity, got {space:?}");
    };
    assert!(total_bytes > 0);
    assert!(available_bytes <= total_bytes);
    assert!(used_bytes <= total_bytes);
}

/// The streaming listing UI renders "Loaded N files..." off these ticks, so a local listing
/// that never calls `on_progress` leaves a big folder on "Opening folder…" for its entire read.
///
/// The ticks are sampled on a timer, so the read holds after its first entry until a tick
/// lands: under load the async side can otherwise wake only after a fast read is done, and see
/// no tick at all (#755).
#[tokio::test]
async fn listing_a_local_directory_reports_progress_while_it_reads() {
    const COUNT: usize = 500;
    let dir = AppTestDir::new("local_listing_progress");
    for i in 0..COUNT {
        std::fs::write(dir.native().join(format!("file_{i:05}.txt")), b"x").expect("writing a scratch file succeeds");
    }

    let (tick_seen, first_tick) = std::sync::mpsc::channel::<()>();
    let first_tick = std::sync::Mutex::new(Some(first_tick));
    let hold_until_a_tick: crate::reading::ListingHook = std::sync::Arc::new(move || {
        // Only the first entry holds; a bulk re-list counting from zero again doesn't.
        if let Some(first_tick) = first_tick.lock_ignore_poison().take() {
            // A safety net for a broken wiring, never the thing the test waits on.
            let _ = first_tick.recv_timeout(std::time::Duration::from_secs(30));
        }
    });
    let volume = volume_at(&dir).with_listing_hook(hold_until_a_tick);
    let ticks: std::sync::Mutex<Vec<ListingProgress>> = std::sync::Mutex::new(Vec::new());
    let on_progress = |p: ListingProgress| {
        ticks.lock_ignore_poison().push(p);
        let _ = tick_seen.send(());
    };

    let entries = volume
        .list_directory(&at(&volume, ""), Some(&on_progress))
        .await
        .expect("listing the scratch dir succeeds");
    assert_eq!(entries.len(), COUNT);

    let ticks = ticks.lock_ignore_poison();
    assert!(
        !ticks.is_empty(),
        "expected a progress tick while the read was held after its first entry"
    );
    for tick in ticks.iter() {
        assert!(
            tick.entries() > 0,
            "a tick with nothing read yet would render 'Loaded 0 files...'"
        );
        assert!(
            tick.entries() <= COUNT,
            "a tick counted more than the folder holds: {} of {COUNT}",
            tick.entries()
        );
    }
    assert!(
        ticks.windows(2).all(|w| w[0].entries() <= w[1].entries()),
        "counts must climb, never go backwards: {:?}",
        ticks.iter().map(ListingProgress::entries).collect::<Vec<_>>()
    );
}
