//! What a finished one-sided local op (a delete, a trash) records in its journal,
//! driven end to end through the starters: how many items it did, and which volume
//! its header and every row name (#1433). An undo reads each row's own volume id, so
//! a header naming the drive over rows naming `root` would be a mismatch of its own.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::{AppTestDir, wait_until_async};
use cmdr_operation_log::store::{open_read_connection, read_operation, read_operation_items};
use cmdr_operation_log::types::Initiator;

use super::OperationEventSink;
use super::event_sinks::CollectorEventSink;
use super::testing::{TestOperationLog, TestOperations};
use super::types::WriteOperationConfig;

const WAIT: Duration = Duration::from_secs(10);

/// A file in a fresh folder, and the folder's app-path text as a `std::path` root.
fn one_file(label: &str) -> (AppTestDir, PathBuf, PathBuf) {
    let dir = AppTestDir::new(label);
    let root = PathBuf::from(dir.app_path().as_str());
    let file = root.join("test_file_1.txt");
    std::fs::write(&file, b"data").expect("write the file");
    (dir, root, file)
}

/// Where the file sits, as the registry sees it.
enum Layout {
    /// Windows: `root` is the system drive alone, and the file's folder is another
    /// drive registered as `drive-2`.
    SecondDrive,
    /// A Mac: `root` holds every local path, a USB drive registered inside it too.
    UnderRoot,
}

/// Registers `root` and the file's own drive the way `layout` says. Returns the
/// system drive's folder, which the caller keeps alive.
fn register(engine: &TestOperations, layout: &Layout, drive_root: &Path, label: &str) -> AppTestDir {
    let system_drive = AppTestDir::new(label);
    let root_folder = match layout {
        Layout::SecondDrive => PathBuf::from(system_drive.app_path().as_str()),
        Layout::UnderRoot => drive_root.parent().expect("the folder has a parent").to_path_buf(),
    };
    let host = crate::host::testing::pane_order_host;
    let volumes = engine.volumes();
    volumes.register(
        "root",
        crate::local_disk::folder_volume("System drive", root_folder, host()),
    );
    let drive_id = match layout {
        Layout::SecondDrive => "drive-2",
        Layout::UnderRoot => "usb-1",
    };
    volumes.register(
        drive_id,
        crate::local_disk::folder_volume("Second drive", drive_root.to_path_buf(), host()),
    );
    system_drive
}

#[derive(Debug)]
struct Header {
    source_volume_id: Option<String>,
    items_done: u64,
    item_count: u64,
    /// Every row's source volume, then every row's dest volume (a trash's in-Trash end).
    row_volumes: Vec<String>,
}

impl Header {
    fn all_on(&self, volume: &str) -> bool {
        self.source_volume_id.as_deref() == Some(volume)
            && !self.row_volumes.is_empty()
            && self.row_volumes.iter().all(|v| v == volume)
    }
}

/// `op_id`'s header and the volumes its rows name.
fn read_header(log: &TestOperationLog, op_id: &str) -> Header {
    let conn = open_read_connection(log.db()).expect("read conn");
    let row = read_operation(&conn, op_id).expect("read op").expect("op row");
    let items = read_operation_items(&conn, op_id, 100).expect("items");
    let volume_of = |dir_id: i64| {
        conn.query_row("SELECT volume_id FROM dirs WHERE dir_id = ?1", [dir_id], |r| {
            r.get::<_, String>(0)
        })
        .expect("dir volume")
    };
    let mut row_volumes: Vec<String> = items.iter().map(|item| volume_of(item.source_dir_id)).collect();
    row_volumes.extend(items.iter().filter_map(|item| item.dest_dir_id).map(volume_of));
    Header {
        source_volume_id: row.source_volume_id,
        items_done: row.items_done,
        item_count: row.item_count,
        row_volumes,
    }
}

async fn settled(events: &CollectorEventSink) {
    wait_until_async(WAIT, "the operation to settle", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
}

/// Deletes `file` permanently through the starter and reads what it journaled.
async fn delete_and_read(engine: &TestOperations, log: &TestOperationLog, file: &Path) -> Header {
    let events = Arc::new(CollectorEventSink::new());
    let started = engine
        .delete_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![crate::testing::app_path_of(file)],
            // The default progress interval (200 ms) outlasts a one-file delete, so no
            // progress frame reaches the status cache: the case that read `0` done.
            WriteOperationConfig::default(),
            None,
            Initiator::User,
            None,
        )
        .await
        .expect("the delete starts");
    settled(&events).await;
    assert!(!file.exists(), "the file is gone");
    read_header(log, &started.operation_id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_delete_reports_the_item_it_deleted() {
    let log = TestOperationLog::new("finished-header-done");
    let engine = TestOperations::journaling_into(&log);
    let (_dir, _root, file) = one_file("finished-header-done");

    let header = delete_and_read(&engine, &log, &file).await;

    assert_eq!(header.item_count, 1, "{header:?}");
    assert_eq!(
        header.items_done, 1,
        "a finished delete never reads as nothing done: {header:?}"
    );
}

/// Windows' shape: the rows and the header name the file's own drive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_on_another_drive_journals_under_that_drive() {
    let log = TestOperationLog::new("finished-header-drive");
    let engine = TestOperations::journaling_into(&log);
    let (_dir, drive_root, file) = one_file("finished-header-drive");
    let _system = register(&engine, &Layout::SecondDrive, &drive_root, "finished-header-system");

    let header = delete_and_read(&engine, &log, &file).await;

    assert!(header.all_on("drive-2"), "{header:?}");
}

/// The Mac's shape: `root` holds every local path, so the rows keep naming `root`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_under_roots_own_root_stays_on_root() {
    let log = TestOperationLog::new("finished-header-root");
    let engine = TestOperations::journaling_into(&log);
    let (_dir, drive_root, file) = one_file("finished-header-root");
    let _system = register(&engine, &Layout::UnderRoot, &drive_root, "finished-header-root-system");

    let header = delete_and_read(&engine, &log, &file).await;

    assert!(header.all_on("root"), "{header:?}");
}

// ── Trash: the OS's own trash, so only where the tests drive a real one ─────────

/// Trashes `file` through the batch starter and reads what it journaled.
#[cfg(any(target_os = "macos", windows))]
async fn batch_trash_and_read(engine: &TestOperations, log: &TestOperationLog, file: &Path) -> Header {
    let _cleanup = crate::testing::TrashCleanup::of(&[file]);
    let events = Arc::new(CollectorEventSink::new());
    let started = engine
        .trash_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![crate::testing::app_path_of(file)],
            None,
            WriteOperationConfig::default(),
            Initiator::User,
            None,
        )
        .await
        .expect("the trash starts");
    settled(&events).await;
    assert!(!file.exists(), "the file went to the trash");
    read_header(log, &started.operation_id)
}

/// Trashes `file` through the single-item path (`move_to_trash`) and reads it back.
#[cfg(any(target_os = "macos", windows))]
fn single_trash_and_read(engine: &TestOperations, log: &TestOperationLog, file: &Path) -> Header {
    let _cleanup = crate::testing::TrashCleanup::of(&[file]);
    engine
        .trash_single_journaled(file, Initiator::User)
        .expect("the trash succeeds");
    let conn = open_read_connection(log.db()).expect("read conn");
    let op_id: String = conn
        .query_row(
            "SELECT op_id FROM operations ORDER BY started_at DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("the trash journaled");
    read_header(log, &op_id)
}

#[cfg(any(target_os = "macos", windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trash_on_another_drive_journals_its_header_and_rows_under_that_drive() {
    let log = TestOperationLog::new("finished-trash-drive");
    let engine = TestOperations::journaling_into(&log);
    let (_dir, drive_root, file) = one_file("finished-trash-drive");
    let _system = register(&engine, &Layout::SecondDrive, &drive_root, "finished-trash-system");

    let header = batch_trash_and_read(&engine, &log, &file).await;

    assert!(header.all_on("drive-2"), "{header:?}");
    assert_eq!(header.items_done, 1, "{header:?}");
}

#[cfg(any(target_os = "macos", windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trash_under_roots_own_root_stays_on_root() {
    let log = TestOperationLog::new("finished-trash-root");
    let engine = TestOperations::journaling_into(&log);
    let (_dir, drive_root, file) = one_file("finished-trash-root");
    let _system = register(&engine, &Layout::UnderRoot, &drive_root, "finished-trash-root-system");

    let header = batch_trash_and_read(&engine, &log, &file).await;

    assert!(header.all_on("root"), "{header:?}");
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_single_trash_on_another_drive_journals_under_that_drive() {
    let log = TestOperationLog::new("finished-single-drive");
    let engine = TestOperations::journaling_into(&log);
    let (_dir, drive_root, file) = one_file("finished-single-drive");
    let _system = register(&engine, &Layout::SecondDrive, &drive_root, "finished-single-system");

    let header = single_trash_and_read(&engine, &log, &file);

    assert!(header.all_on("drive-2"), "{header:?}");
    assert_eq!(header.items_done, 1, "{header:?}");
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_single_trash_under_roots_own_root_stays_on_root() {
    let log = TestOperationLog::new("finished-single-root");
    let engine = TestOperations::journaling_into(&log);
    let (_dir, drive_root, file) = one_file("finished-single-root");
    let _system = register(&engine, &Layout::UnderRoot, &drive_root, "finished-single-root-system");

    let header = single_trash_and_read(&engine, &log, &file);

    assert!(header.all_on("root"), "{header:?}");
}
