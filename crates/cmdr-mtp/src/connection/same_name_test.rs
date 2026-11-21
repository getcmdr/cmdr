//! Two siblings answering to one name.
//!
//! PTP lets a phone or camera hold two files with the same name in one folder.
//! ❗ Regression anchor for #523: the path cache kept the LAST twin, so a backup
//! read the second twin twice and never the first, and deleting row one deleted
//! the other file. Each twin's row now carries its own object, so every op on a
//! row reaches exactly that row's file, and only the shared name given alone (a
//! path with no row behind it) is refused.
//!
//! The virtual device is backed by a real folder, which can't hold twins, so
//! `testing::make_twins` has `report.txt` answer to `notes.txt` too. Its backing
//! file keeps its own name, which is how a cell tells which object an op hit.

use super::testing::test_connection_manager as connection_manager;
use super::{MtpConnectionError, MtpDeleteScope};
use crate::testing::{ConnectedDevice, connect_virtual_device, device_lock, make_twins};
use cmdr_fs::entry::FileEntry;

const NOTES: &str = "Meeting notes: discuss MTP E2E test strategy.\n";
const REPORT: &str = "Quarterly report: Q4 2025 placeholder content.\n";

/// The two `notes.txt` rows `/Documents` lists once `report.txt` answers to that
/// name too: `(the real notes.txt's row, report.txt's row)`.
async fn twin_rows(device: &ConnectedDevice) -> (FileEntry, FileEntry) {
    let manager = connection_manager();
    let (notes, report) = make_twins(manager, device, "/Documents", "notes.txt", "report.txt").await;
    let entries = manager
        .list_directory(&device.id, device.storage_id, "/Documents")
        .await
        .expect("list Documents should succeed");
    let row = |handle: u64| {
        entries
            .iter()
            .find(|entry| entry.inode == Some(handle))
            .cloned()
            .expect("each twin lists as its own row")
    };
    let (first, second) = (row(notes), row(report));
    assert_eq!(
        (first.name.as_str(), second.name.as_str()),
        ("notes.txt", "notes.txt"),
        "both rows show the name they share"
    );
    assert_ne!(first.path, second.path, "each twin's row needs its own path");
    (first, second)
}

async fn read_all(device: &ConnectedDevice, path: &str) -> Result<String, MtpConnectionError> {
    let bytes = connection_manager()
        .read_range_direct(&device.id, device.storage_id, path, 0, 64 * 1024)
        .await?;
    Ok(String::from_utf8(bytes).expect("the fixture files are text"))
}

fn on_disk(device: &ConnectedDevice, name: &str) -> Option<String> {
    std::fs::read_to_string(device.root().join("internal/Documents").join(name)).ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_twin_row_reads_its_own_bytes_and_the_shared_name_alone_is_refused() {
    let _guard = device_lock().await;
    let device = connect_virtual_device(connection_manager()).await;
    let (notes, report) = twin_rows(&device).await;

    assert_eq!(read_all(&device, &notes.path).await.ok().as_deref(), Some(NOTES));
    assert_eq!(read_all(&device, &report.path).await.ok().as_deref(), Some(REPORT));
    let shared = read_all(&device, "/Documents/notes.txt").await;
    assert!(
        matches!(&shared, Err(MtpConnectionError::AmbiguousName { path, .. }) if path == "/Documents/notes.txt"),
        "the name alone names no one object, got {shared:?}"
    );

    device.teardown(connection_manager()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_twin_row_deletes_that_object_and_leaves_the_other() {
    let _guard = device_lock().await;
    let device = connect_virtual_device(connection_manager()).await;
    let (notes, _report) = twin_rows(&device).await;

    connection_manager()
        .delete_object(&device.id, device.storage_id, &notes.path, MtpDeleteScope::SingleNode)
        .await
        .expect("deleting row one should succeed");

    assert_eq!(on_disk(&device, "notes.txt"), None, "row one's object is gone");
    assert_eq!(
        on_disk(&device, "report.txt").as_deref(),
        Some(REPORT),
        "row two's stays"
    );

    device.teardown(connection_manager()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn renaming_a_twin_row_renames_that_object_and_leaves_the_other() {
    let _guard = device_lock().await;
    let device = connect_virtual_device(connection_manager()).await;
    let (_notes, report) = twin_rows(&device).await;

    let renamed = connection_manager()
        .rename_object(&device.id, device.storage_id, &report.path, "report-2.txt")
        .await
        .expect("renaming row two should succeed");

    assert_eq!(
        renamed.path, "/Documents/report-2.txt",
        "a renamed twin is an ordinary row again"
    );
    assert_eq!(on_disk(&device, "report-2.txt").as_deref(), Some(REPORT));
    assert_eq!(
        on_disk(&device, "notes.txt").as_deref(),
        Some(NOTES),
        "row one's object is untouched"
    );

    device.teardown(connection_manager()).await;
}

/// ❗ A device that names an object `..`, ``, `.`, or `a/b` lists no row for it: its row's
/// path would have named the folder's parent, the folder itself, or an object in another
/// folder, so a delete of that row acted on something nobody picked. The rest of the
/// folder lists as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_object_no_path_can_address_lists_no_row() {
    let _guard = device_lock().await;
    let manager = connection_manager();
    let device = connect_virtual_device(manager).await;
    let entries = manager
        .list_directory(&device.id, device.storage_id, "/Documents")
        .await
        .expect("list Documents should succeed");
    let report = entries
        .iter()
        .find(|entry| entry.name == "report.txt")
        .and_then(|entry| entry.inode)
        .expect("the fixture holds report.txt");

    for name in ["..", "", ".", "a/b"] {
        manager
            .report_object_as_for_test(&device.id, mtp_rs::ObjectHandle(report), name)
            .await;
        let listed = manager
            .list_directory(&device.id, device.storage_id, "/Documents")
            .await
            .expect("the folder still lists");
        assert!(
            listed.iter().all(|entry| entry.inode != Some(report)),
            "an object named {name:?} must be no row, got {:?}",
            listed
                .iter()
                .map(|entry| (&entry.name, &entry.path))
                .collect::<Vec<_>>()
        );
        assert!(
            listed
                .iter()
                .any(|entry| entry.name == "notes.txt" && entry.path == "/Documents/notes.txt"),
            "the rest of the folder lists as before, with {name:?} beside it"
        );
    }

    device.teardown(manager).await;
}
