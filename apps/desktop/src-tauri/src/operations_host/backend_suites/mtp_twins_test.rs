//! Two items with one name in a phone or camera folder, copied off through the
//! app's whole pipeline (#523). ❗ Regression anchor: both rows resolved to the
//! last twin, so a backup read it twice and never the first. Each row is its
//! own object now, and the second lands on a taken name, so it goes through the
//! conflict policy like any other clash.
//!
//! `make_twins` has `report.txt` answer to `notes.txt` too (the virtual device's
//! real folder can't hold twins), so each twin's bytes tell which one landed.

use cmdr_fs::app_path;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_fs::volume::Volume;
use cmdr_local::LocalVolume;
use cmdr_operations::{ConflictResolution, VolumeCopyConfig};

use super::network_transfer_test_support::start_copy_on;
use crate::commands::wire_path::as_sent_std_path;
use crate::mtp::test_support::{connect_virtual_device, device_lock, make_twins, teardown, volume_for};
use crate::operations_host::app_engine;
use crate::test_support::TestDir;

const NOTES: &str = "Meeting notes: discuss MTP E2E test strategy.\n";
const REPORT: &str = "Quarterly report: Q4 2025 placeholder content.\n";

/// What every file in `dir` whose name starts with `notes` holds.
fn notes_landed_in(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("notes"))
        })
        .map(|path| std::fs::read_to_string(&path).expect("a landed copy is readable"))
        .collect()
}

/// Copies `sources` off the device into a fresh local folder, renaming on a
/// clash, and answers that folder.
async fn copy_off(label: &str, device_volume: Arc<dyn Volume>, sources: Vec<PathBuf>) -> TestDir {
    let local_dir = TestDir::new(label);
    let local: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Local",
        &*local_dir,
        crate::volume_host::host(),
    ));
    let running = start_copy_on(
        app_engine(),
        label,
        device_volume,
        sources,
        local,
        PathBuf::from(""),
        VolumeCopyConfig {
            conflict_resolution: ConflictResolution::Rename,
            ..VolumeCopyConfig::default()
        },
    )
    .await;
    running.settle().await;
    running.assert_no_errors();
    local_dir
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copying_a_folder_holding_twins_off_a_device_keeps_both() {
    let _guard = device_lock().await;
    let device = connect_virtual_device().await;
    let volume: Arc<dyn Volume> = Arc::new(volume_for(&device, None).await);
    make_twins(&device, "/Documents", "notes.txt", "report.txt").await;

    let local_dir = copy_off("mtp_twins_folder_off", volume, vec![PathBuf::from("/Documents")]).await;

    assert_eq!(
        notes_landed_in(&local_dir.join("Documents")),
        BTreeSet::from([NOTES.to_string(), REPORT.to_string()]),
        "each twin lands once, the second under a free name"
    );

    teardown(device).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copying_both_selected_twin_rows_off_a_device_keeps_both() {
    let _guard = device_lock().await;
    let device = connect_virtual_device().await;
    let mtp_volume = volume_for(&device, None).await;
    make_twins(&device, "/Documents", "notes.txt", "report.txt").await;
    let rows: Vec<PathBuf> = mtp_volume
        .list_directory(app_path!("/Documents"), None)
        .await
        .expect("listing Documents should succeed")
        .into_iter()
        .filter(|entry| entry.name == "notes.txt")
        // The engine's starts take a row's text as a `PathBuf` until they take app paths.
        .map(|entry| as_sent_std_path(&entry.path))
        .collect();
    assert_eq!(rows.len(), 2, "both twins list");

    let local_dir = copy_off("mtp_twins_rows_off", Arc::new(mtp_volume), rows).await;

    assert_eq!(
        notes_landed_in(&local_dir),
        BTreeSet::from([NOTES.to_string(), REPORT.to_string()]),
        "each twin lands once, the second under a free name"
    );

    teardown(device).await;
}

/// Bulk-renames `rows` (source row path, destination) on `volume` through the
/// app's engine and waits for it to settle; answers how many rows it skipped.
async fn bulk_rename(volume: Arc<dyn Volume>, rows: Vec<(PathBuf, PathBuf)>) -> usize {
    use cmdr_fs::ignore_poison::IgnorePoison;
    use cmdr_operation_log::types::Initiator;
    use cmdr_operations::testing::CollectorEventSink;
    use cmdr_operations::{BulkRenameRow, SourceFingerprint};

    let registered = super::network_safety_test_support::Registered::new(&volume, "mtp-twins-rename");
    let mut bulk_rows = Vec::new();
    for (source, destination) in rows {
        bulk_rows.push(BulkRenameRow {
            row_id: bulk_rows.len().to_string(),
            expected_fingerprint: SourceFingerprint::capture_remote(volume.as_ref(), &source)
                .await
                .expect("a twin row fingerprints as its own file"),
            source,
            destination,
        });
    }
    let events = Arc::new(CollectorEventSink::new());
    cmdr_operations::testing::start_bulk_rename(
        app_engine(),
        events.clone() as Arc<dyn cmdr_operations::OperationEventSink>,
        registered.id.clone(),
        bulk_rows,
        Initiator::User,
    )
    .expect("a rename of a twin in its own folder starts");
    crate::test_support::wait_until_async(std::time::Duration::from_secs(10), "the bulk rename to settle", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
    let skipped = events
        .complete
        .lock_ignore_poison()
        .first()
        .map_or(0, |complete| complete.files_skipped);
    registered.leave();
    skipped
}

/// The rows `/Documents` lists under `notes.txt`: `(the real notes.txt's, report.txt's)`.
async fn twin_rows(volume: &dyn Volume, handles: (u64, u64)) -> (PathBuf, PathBuf) {
    let entries = volume
        .list_directory(app_path!("/Documents"), None)
        .await
        .expect("listing Documents should succeed");
    let row = |handle| {
        entries
            .iter()
            .find(|entry| entry.inode == Some(handle))
            .map(|entry| as_sent_std_path(&entry.path))
            .expect("each twin lists as its own row")
    };
    (row(handles.0), row(handles.1))
}

fn on_disk(device: &crate::mtp::test_support::ConnectedDevice, name: &str) -> Option<String> {
    std::fs::read_to_string(device.root().join("internal/Documents").join(name)).ok()
}

/// ❗ A twin's row path and its plain destination are ONE folder: the start
/// compared `Path::parent`s and refused the batch as a cross-folder rename.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bulk_rename_of_a_twin_row_renames_that_file_in_its_own_folder() {
    let _guard = device_lock().await;
    let device = connect_virtual_device().await;
    let volume: Arc<dyn Volume> = Arc::new(volume_for(&device, None).await);
    let handles = make_twins(&device, "/Documents", "notes.txt", "report.txt").await;
    let (_notes, report) = twin_rows(volume.as_ref(), handles).await;

    let skipped = bulk_rename(
        Arc::clone(&volume),
        vec![(report, PathBuf::from("/Documents/report-2.txt"))],
    )
    .await;

    assert_eq!(skipped, 0);
    assert_eq!(on_disk(&device, "report-2.txt").as_deref(), Some(REPORT));
    assert_eq!(
        on_disk(&device, "notes.txt").as_deref(),
        Some(NOTES),
        "the other twin is untouched"
    );

    teardown(device).await;
}

/// ❗ A destination spelled under the twin's pick still meets the folder's
/// names: a taken one is skipped, never a second item under that name.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bulk_rename_onto_a_taken_name_under_a_twins_pick_is_skipped() {
    let _guard = device_lock().await;
    let device = connect_virtual_device().await;
    let volume: Arc<dyn Volume> = Arc::new(volume_for(&device, None).await);
    let handles = make_twins(&device, "/Documents", "notes.txt", "report.txt").await;
    volume
        .create_directory(app_path!("/Documents/taken"))
        .await
        .expect("making the clashing folder");
    let (_notes, report) = twin_rows(volume.as_ref(), handles).await;
    let under_the_pick = report.parent().expect("a row has a parent").join("taken");

    let skipped = bulk_rename(Arc::clone(&volume), vec![(report, under_the_pick)]).await;

    assert_eq!(skipped, 1, "the name is taken");
    assert_eq!(on_disk(&device, "report.txt").as_deref(), Some(REPORT), "nothing moved");

    teardown(device).await;
}
