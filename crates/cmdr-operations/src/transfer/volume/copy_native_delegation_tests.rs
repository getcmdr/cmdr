//! A copy between two volumes that both sit on this machine's own disk goes to the
//! local engine (`copy_between_volumes`'s native shortcut), which reads files
//! itself, so it never needs the source's `Volume` streams. Windows' local volume
//! has no export streams yet, and every local copy there refused with "does not
//! support export" before reaching the shortcut (2026-11-21, on Mami).

use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::TestDir;
use cmdr_local::LocalVolume;

/// A real local volume that offers no export streams, as Windows' does today.
struct NoExportStreams {
    inner: LocalVolume,
}

impl Volume for NoExportStreams {
    crate::forward_volume_methods!(inner =>
        name, root, lane_key, list_directory, get_metadata, exists, is_directory, create_file,
        create_directory, create_directory_all, delete, rename, get_space_info, local_path,
        supports_local_fs_access, operations_are_local, scan_for_copy, scan_for_copy_batch,
        scan_for_conflicts
    );
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn supports_export(&self) -> bool {
        false
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copy_between_two_local_volumes_needs_no_export_streams() {
    let src_dir = TestDir::new("native_delegation_src");
    let dst_dir = TestDir::new("native_delegation_dst");
    std::fs::write(src_dir.join("notes.txt"), "NEW").unwrap();
    let source: Arc<dyn Volume> = Arc::new(NoExportStreams {
        inner: LocalVolume::local_folder(
            "Source",
            src_dir.to_str().unwrap(),
            crate::host::testing::pane_order_host(),
        ),
    });
    let dest: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Dest",
        dst_dir.to_str().unwrap(),
        crate::host::testing::pane_order_host(),
    ));

    let collector = Arc::new(CollectorEventSink::new());
    let events: Arc<dyn OperationEventSink> = collector.clone();
    TestOperations::new()
        .copy_between_volumes(
            events,
            "source".to_string(),
            source,
            // Absolute, as the app sends a local source (`on_local_root`: an absolute path wins).
            vec![src_dir.join("notes.txt")],
            "dest".to_string(),
            dest,
            PathBuf::from("/"),
            VolumeCopyConfig::default(),
            cmdr_operation_log::types::Initiator::User,
            None,
        )
        .await
        .expect("a local-to-local copy starts without export streams");

    cmdr_fs::testing::wait_until_async(Duration::from_secs(10), "the copy to settle", || {
        !collector.settled.lock_ignore_poison().is_empty()
    })
    .await;
    assert!(
        collector.errors.lock_ignore_poison().is_empty(),
        "{:?}",
        collector.errors.lock_ignore_poison()
    );
    assert_eq!(std::fs::read_to_string(dst_dir.join("notes.txt")).unwrap(), "NEW");
}
