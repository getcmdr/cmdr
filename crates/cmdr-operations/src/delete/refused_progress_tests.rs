//! A delete the OS stops partway says how far it got: the error dialog words `progressAtStop`, and
//! a delete can't be undone, so "how much is already gone" is the one thing the person needs.
#![cfg(unix)] // POSIX modes make the refusing folder.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::{TestDir, wait_until_async};
use cmdr_operation_log::types::Initiator;

use super::super::OperationEventSink;
use super::super::event_sinks::CollectorEventSink;
use super::super::testing::TestOperations;
use super::super::types::{WriteOperationConfig, WriteOperationError};

/// ERR-RB2XQ (cmdr-reports#122): a delete of an app whose nested folder was root-owned stopped
/// on `EACCES` 2 ms in. The walker's counts reach the status cache only on a progress frame
/// (200 ms apart), and `progress_at_stop` reads that cache, so a refusal inside the first frame
/// reported nothing done, however many files were already gone for good.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_delete_reports_every_file_it_already_removed() {
    let dir = TestDir::new("refused_delete_progress");
    let root = dir.join("Test App.app");
    let open = root.join("a-open");
    let closed = root.join("b-closed");
    fs::create_dir_all(&open).expect("a writable folder");
    fs::create_dir_all(&closed).expect("a folder to close");
    for name in ["1.txt", "2.txt", "3.txt"] {
        fs::write(open.join(name), b"x").expect("a file it can delete");
    }
    fs::write(closed.join("kept.txt"), b"x").expect("a file it can't");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o555)).expect("close the folder");

    let events = Arc::new(CollectorEventSink::new());
    TestOperations::new()
        .delete_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            // Two sources, so the walker meets the open folder first: the order inside one
            // folder is the directory's own.
            vec![crate::testing::app_path_of(&open), crate::testing::app_path_of(&closed)],
            // The default 200 ms progress interval outlasts this whole delete.
            WriteOperationConfig::default(),
            None,
            Initiator::User,
            None,
        )
        .await
        .expect("the delete starts");
    wait_until_async(Duration::from_secs(5), "the delete to settle", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).expect("reopen the folder");

    let errors = events.errors.lock_ignore_poison();
    let [event] = errors.as_slice() else {
        panic!("one write-error, got {errors:?}");
    };
    assert!(
        matches!(event.error, WriteOperationError::PermissionDenied { .. }),
        "{:?}",
        event.error
    );
    let gone = ["1.txt", "2.txt", "3.txt"]
        .iter()
        .filter(|name| !open.join(name).exists())
        .count();
    assert_eq!(
        gone, 3,
        "the walker reached the open folder first, so this pins something"
    );
    let progress = event
        .progress_at_stop
        .as_ref()
        .expect("a stopped delete says how far it got");
    assert_eq!(progress.files_done as usize, gone, "{progress:?}");
    assert_eq!(progress.files_total, 4, "{progress:?}");
}
