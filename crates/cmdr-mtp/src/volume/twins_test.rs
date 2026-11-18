//! Same-named siblings through `MtpVolume`: what a pane, a copy, a rename, and
//! a delete reach when a folder holds two items with one name (#523).
//!
//! `testing::make_twins` has `report.txt` answer to `notes.txt` too (the
//! virtual device's real folder can't hold twins), so its backing file keeps
//! its own name and a cell reads which object an op hit off the disk.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};

use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{Volume, VolumeError};

use crate::testing::{
    ConnectedDevice, connect_virtual_device, device_lock, make_twins, test_connection_manager, volume_for,
};
use crate::volume::MtpVolume;

const NOTES: &str = "Meeting notes: discuss MTP E2E test strategy.\n";
const REPORT: &str = "Quarterly report: Q4 2025 placeholder content.\n";

/// A device whose `/Documents` holds two `notes.txt`, a volume over it, and
/// the two rows the volume lists: `(the real notes.txt's, report.txt's)`.
async fn twins_on_a_volume() -> (ConnectedDevice, MtpVolume, FileEntry, FileEntry) {
    let device = connect_virtual_device(test_connection_manager()).await;
    let volume = volume_for(test_connection_manager(), &device, None).await;
    let (notes, report) = make_twins(
        test_connection_manager(),
        &device,
        "/Documents",
        "notes.txt",
        "report.txt",
    )
    .await;
    let entries = volume
        .list_directory(app_path!("/Documents"), None)
        .await
        .expect("listing Documents should succeed");
    let row = |handle: u64| {
        entries
            .iter()
            .find(|entry| entry.inode == Some(handle))
            .cloned()
            .expect("each twin lists as its own row")
    };
    let (first, second) = (row(notes), row(report));
    (device, volume, first, second)
}

fn on_disk(device: &ConnectedDevice, name: &str) -> Option<String> {
    std::fs::read_to_string(device.root().join("internal/Documents").join(name)).ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_twin_row_stats_as_its_own_object_and_the_shared_name_stats_as_taken() {
    let _guard = device_lock().await;
    let (device, volume, notes, report) = twins_on_a_volume().await;

    let stat = |path: String| {
        let volume = &volume;
        async move { volume.get_metadata(&AppPath::legacy_unchecked(Path::new(&path))).await }
    };
    let notes_stat = stat(notes.path.clone()).await.expect("row one stats");
    let report_stat = stat(report.path.clone()).await.expect("row two stats");
    assert_eq!(
        (notes_stat.inode, notes_stat.size),
        (notes.inode, Some(NOTES.len() as u64))
    );
    assert_eq!(
        (report_stat.inode, report_stat.size),
        (report.inode, Some(REPORT.len() as u64))
    );

    let shared = Path::new("/Documents/notes.txt");
    let outcome = volume.get_metadata(&AppPath::legacy_unchecked(shared)).await;
    assert!(
        matches!(&outcome, Err(VolumeError::DuplicateName(_))),
        "the name alone names no one object, got {outcome:?}"
    );
    assert!(
        volume.exists(&AppPath::legacy_unchecked(shared)).await,
        "a name two items hold is taken, so nothing may land on it unasked"
    );

    device.teardown(test_connection_manager()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copy_scan_of_both_twin_rows_counts_each_object() {
    let _guard = device_lock().await;
    let (device, volume, notes, report) = twins_on_a_volume().await;

    let paths = [PathBuf::from(&notes.path), PathBuf::from(&report.path)];
    let scan = volume
        .scan_for_copy_batch(
            &paths
                .iter()
                .map(|p| AppPath::legacy_unchecked(p).into_owned())
                .collect::<Vec<_>>(),
        )
        .await
        .expect("the scan should succeed");

    assert_eq!(scan.aggregate.file_count, 2);
    assert_eq!(scan.aggregate.total_bytes, (NOTES.len() + REPORT.len()) as u64);
    assert_eq!(scan.per_path.len(), 2, "each selected row gets its own answer");

    device.teardown(test_connection_manager()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn renaming_a_twin_row_through_the_volume_renames_that_object() {
    let _guard = device_lock().await;
    let (device, volume, _notes, report) = twins_on_a_volume().await;

    // The rename editor names the target after the pane's folder.
    volume
        .rename(
            &AppPath::legacy_unchecked(Path::new(&report.path)),
            app_path!("/Documents/report-2.txt"),
            false,
        )
        .await
        .expect("renaming row two should succeed");

    assert_eq!(on_disk(&device, "report-2.txt").as_deref(), Some(REPORT));
    assert_eq!(on_disk(&device, "notes.txt").as_deref(), Some(NOTES));

    device.teardown(test_connection_manager()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_twin_row_through_the_volume_deletes_that_object() {
    let _guard = device_lock().await;
    let (device, volume, notes, _report) = twins_on_a_volume().await;

    volume
        .delete(&AppPath::legacy_unchecked(Path::new(&notes.path)))
        .await
        .expect("deleting row one should succeed");

    assert_eq!(on_disk(&device, "notes.txt"), None);
    assert_eq!(on_disk(&device, "report.txt").as_deref(), Some(REPORT));

    device.teardown(test_connection_manager()).await;
}

/// ❗ A destination built as `Path::parent(row).join(new)` carries the twin's
/// pick, and must still see a name the folder holds: else the no-clobber check
/// reads it as free and the device adds a second item under that name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_spelled_under_a_twins_pick_still_sees_a_taken_name() {
    let _guard = device_lock().await;
    let (device, volume, _notes, report) = twins_on_a_volume().await;
    volume
        .create_directory(app_path!("/Documents/taken"))
        .await
        .expect("making the clashing folder");
    let under_the_pick = Path::new(&report.path)
        .parent()
        .expect("a row has a parent")
        .join("taken");

    let outcome = volume
        .rename(
            &AppPath::legacy_unchecked(Path::new(&report.path)),
            &AppPath::legacy_unchecked(&under_the_pick),
            false,
        )
        .await;

    assert!(
        matches!(&outcome, Err(VolumeError::AlreadyExists(_))),
        "the name is taken, got {outcome:?}"
    );
    assert_eq!(on_disk(&device, "report.txt").as_deref(), Some(REPORT), "nothing moved");

    device.teardown(test_connection_manager()).await;
}
