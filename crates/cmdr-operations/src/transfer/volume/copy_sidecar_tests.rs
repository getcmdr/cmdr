//! A cross-volume copy out of a local FAT or exFAT folder leaves each item's
//! AppleDouble sidecar behind, the way the native engine's walk does
//! (`cmdr_fs::apple_double`). The source plays a FAT volume on APFS through an
//! injected probe, since no automated test may attach a FAT image.
#![cfg(unix)] // POSIX-only cells: `LocalPosixVolume`, POSIX modes, or errnos (windows-port plan, tasks 35 and 36).

use cmdr_fs::app_path::AppPath;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::copy_volumes_with_progress;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::VolumeCopyConfig;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_local::LocalPosixVolume;

/// `folder/` holding `child.txt` with its sidecar, a subfolder with its own,
/// and an orphan sidecar whose owner is gone.
fn fat_source(scratch: &Path) -> Arc<LocalPosixVolume> {
    let folder = scratch.join("folder");
    fs::create_dir_all(folder.join("sub")).expect("fixture tree");
    fs::write(folder.join("child.txt"), b"child").expect("the owner");
    fs::write(folder.join("._child.txt"), [0u8; 4096]).expect("its sidecar");
    fs::write(folder.join("._sub"), [0u8; 4096]).expect("the subfolder's sidecar");
    fs::write(folder.join("._orphan.txt"), b"orphan").expect("an orphan");
    fs::write(folder.join("sub/inner.txt"), b"inner").expect("a nested file");
    Arc::new(
        LocalPosixVolume::local_folder("FAT", scratch, crate::host::testing::pane_order_host())
            .with_sidecar_probe(|_| true),
    )
}

async fn names_in(volume: &dyn Volume, dir: &str) -> Vec<String> {
    let mut names: Vec<String> = volume
        .list_directory(AppPath::from_normalized(dir).expect("a canonical path"), None)
        .await
        .expect("the landed folder lists")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copy_out_of_a_fat_folder_leaves_paired_sidecars_behind() {
    let engine = TestOperations::new();
    let scratch = TestDir::new("volume_sidecar_copy");
    let source = fat_source(&scratch);
    let dest = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-volume-sidecar-copy",
        &Arc::new(engine.new_operation_state(Duration::from_millis(50))),
        Arc::clone(&source) as Arc<dyn Volume>,
        &[PathBuf::from("folder")],
        Arc::clone(&dest) as Arc<dyn Volume>,
        Path::new(""),
        &VolumeCopyConfig::default(),
    )
    .await
    .unwrap_or_else(|failure| panic!("the copy succeeds, got {:?}", failure.error));

    assert_eq!(names_in(&*dest, "/folder").await, ["._orphan.txt", "child.txt", "sub"]);
    assert_eq!(names_in(&*dest, "/folder/sub").await, ["inner.txt"]);
}

/// The scan the copy dialog shows counts what the copy will write.
#[tokio::test]
async fn the_scan_of_a_fat_folder_counts_no_sidecars() {
    let scratch = TestDir::new("volume_sidecar_scan");
    let source = fat_source(&scratch);

    let scan = source
        .scan_for_copy(&AppPath::legacy_unchecked(Path::new("folder")))
        .await
        .expect("the scan runs");

    assert_eq!(scan.file_count, 3, "child, inner, and the orphan");
    assert_eq!(scan.dir_count, 1, "sub");
    assert_eq!(scan.total_bytes, 5 + 5 + 6);
}

/// The selected sidecar is the person's to copy, and on a volume with native
/// xattrs no `._` file is anyone's sidecar.
#[tokio::test]
async fn a_selected_sidecar_and_a_native_volume_keep_everything() {
    let scratch = TestDir::new("volume_sidecar_selected");
    let source = fat_source(&scratch);
    let selected = source
        .scan_for_copy(&AppPath::legacy_unchecked(Path::new("folder/._child.txt")))
        .await
        .expect("scan");
    assert_eq!((selected.file_count, selected.total_bytes), (1, 4096));

    let native = LocalPosixVolume::local_folder("APFS", &*scratch, crate::host::testing::pane_order_host())
        .with_sidecar_probe(|_| false);
    let listed = native
        .list_directory_as_source(&AppPath::legacy_unchecked(Path::new("folder")))
        .await
        .expect("the folder lists");
    assert_eq!(listed.len(), 5, "all five entries, sidecars included");
}
