//! What a finished one-sided local op (a delete, a trash) records in its journal
//! header, driven end to end through the starter: how many items it did, and which
//! volume it ran on (#1433).

use std::path::PathBuf;
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

/// Deletes `file` permanently through the starter and waits for it to settle.
/// Returns `(source_volume_id, items_done, item_count, the item rows' volumes)`.
async fn delete_and_read(engine: &TestOperations, log: &TestOperationLog, file: &std::path::Path) -> Header {
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
    wait_until_async(WAIT, "the delete to settle", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
    assert!(!file.exists(), "the file is gone");

    let conn = open_read_connection(log.db()).expect("read conn");
    let row = read_operation(&conn, &started.operation_id)
        .expect("read op")
        .expect("op row");
    let items = read_operation_items(&conn, &started.operation_id, 100).expect("items");
    let row_volumes = items
        .iter()
        .map(|item| {
            conn.query_row(
                "SELECT volume_id FROM dirs WHERE dir_id = ?1",
                [item.source_dir_id],
                |r| r.get::<_, String>(0),
            )
            .expect("dir volume")
        })
        .collect();
    Header {
        source_volume_id: row.source_volume_id,
        items_done: row.items_done,
        item_count: row.item_count,
        row_volumes,
    }
}

#[derive(Debug)]
struct Header {
    source_volume_id: Option<String>,
    items_done: u64,
    item_count: u64,
    row_volumes: Vec<String>,
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

/// Windows' shape: `root` is the system drive alone, and the file sits on another
/// drive registered under its own id. The rows and the header name that drive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_on_another_drive_journals_under_that_drive() {
    let log = TestOperationLog::new("finished-header-drive");
    let engine = TestOperations::journaling_into(&log);
    let system_drive = AppTestDir::new("finished-header-system-drive");
    let (_dir, drive_root, file) = one_file("finished-header-drive");
    let volumes = engine.volumes();
    volumes.register(
        "root",
        crate::local_disk::folder_volume(
            "System drive",
            PathBuf::from(system_drive.app_path().as_str()),
            crate::host::testing::pane_order_host(),
        ),
    );
    volumes.register(
        "drive-2",
        crate::local_disk::folder_volume("Second drive", drive_root, crate::host::testing::pane_order_host()),
    );

    let header = delete_and_read(&engine, &log, &file).await;

    assert_eq!(header.source_volume_id.as_deref(), Some("drive-2"), "{header:?}");
    assert!(!header.row_volumes.is_empty(), "{header:?}");
    assert!(header.row_volumes.iter().all(|v| v == "drive-2"), "{header:?}");
}

/// The Mac's shape: `root` is `/` and holds every local path, a USB drive's included,
/// so the rows keep naming `root` as they always did.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_under_roots_own_root_stays_on_root() {
    let log = TestOperationLog::new("finished-header-root");
    let engine = TestOperations::journaling_into(&log);
    let (_dir, drive_root, file) = one_file("finished-header-root");
    let holding = drive_root.parent().expect("the folder has a parent").to_path_buf();
    let volumes = engine.volumes();
    volumes.register(
        "root",
        crate::local_disk::folder_volume("Local disk", holding, crate::host::testing::pane_order_host()),
    );
    volumes.register(
        "usb-1",
        crate::local_disk::folder_volume("USB drive", drive_root, crate::host::testing::pane_order_host()),
    );

    let header = delete_and_read(&engine, &log, &file).await;

    assert_eq!(header.source_volume_id.as_deref(), Some("root"), "{header:?}");
    assert!(header.row_volumes.iter().all(|v| v == "root"), "{header:?}");
}
