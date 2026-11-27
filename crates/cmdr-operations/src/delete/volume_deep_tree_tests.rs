//! A volume delete of a folder tree of any depth scans and deletes it on the
//! stack a delete really runs on (#1541): the scan keeps the folders it's inside
//! on the heap, so a deep tree can't overflow the 2 MiB tokio worker. A scan
//! that nests a future per level aborts this test binary instead.
#![cfg(unix)] // A 400-level path outgrows a Windows path without the long-path prefix.

use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::Volume;
use cmdr_local::LocalVolume;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::super::event_sinks::CollectorEventSink;
use super::super::testing::TestOperations;
use super::super::types::WriteOperationConfig;
use super::volume_walker::delete_volume_files_with_progress_inner;

/// Inside macOS's 1,024-byte `PATH_MAX` with the temp folder in front: each
/// level adds two bytes (`d/`).
const DEPTH: usize = 400;

#[test]
fn a_four_hundred_level_tree_deletes_on_a_workers_stack() {
    let dir = TestDir::new("deep-tree-delete");
    let bottom = dir.join(format!("deep{}", "/d".repeat(DEPTH)));
    std::fs::create_dir_all(&bottom).unwrap();
    std::fs::write(bottom.join("bottom.txt"), b"bottom").unwrap();
    let volume: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Local",
        dir.to_path_buf(),
        crate::host::testing::pane_order_host(),
    ));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(2 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap();

    let result = runtime
        .block_on(runtime.spawn(async move {
            let state = Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(50)));
            delete_volume_files_with_progress_inner(
                volume,
                "deep-tree-volume",
                &CollectorEventSink::new(),
                "op-deep-delete",
                &state,
                &[PathBuf::from("deep")],
                &WriteOperationConfig::default(),
            )
            .await
        }))
        .expect("the delete's task ends without a panic");

    assert!(result.is_ok(), "{result:?}");
    assert!(!dir.join("deep").exists(), "every level went");
}
