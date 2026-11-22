//! Undo of a transfer that landed under an explicit leaf (`transfer/DETAILS.md`
//! § Named destinations), through the real journal and the real rollback gate:
//! a named copy's undo deletes exactly the named copy, and a named move's undo
//! puts the original name back. The named LOCAL move's undo lives in
//! `transfer/move_op/move_journal_tests.rs`.
//!
//! Each fixture keeps a bystander under the source's ORIGINAL name at the
//! destination, so an undo that read the original name instead of the landed
//! one would touch the wrong file.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::event_sinks::CollectorEventSink;
use super::journal;
use super::transfer::copy::copy_files_with_progress_inner;
use super::types::{VolumeCopyConfig, WriteOperationConfig};
use super::{copy_volumes_with_progress, move_volumes_with_progress, move_within_same_volume_with_progress};
use crate::host::testing::FakeVolumes;
use crate::rollback::Reversal;
use crate::testing::{TestOperationLog, TestOperations};
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_operation_log::rollback::{RollbackReport, execute_rollback, rollback_operation};
use cmdr_operation_log::types::{ExecutionStatus, Initiator, OpKind};

/// An engine journaling into a temp DB, and the volumes the rollback reaches.
struct UndoRig {
    engine: TestOperations,
    log: TestOperationLog,
    vm: FakeVolumes,
}

impl UndoRig {
    fn new() -> Self {
        let log = TestOperationLog::new("named-leaf-undo");
        UndoRig {
            engine: TestOperations::journaling_into(&log),
            log,
            vm: FakeVolumes::default(),
        }
    }

    /// Ask for the reversal the way the app does: the gate decides, then the engine runs.
    async fn undo(&self, op_id: &str) -> RollbackReport {
        let writer = self.log.writer();
        let plan = rollback_operation(&self.vm, writer, op_id, |_plan| Ok(())).expect("the named transfer reverses");
        let reversal = Reversal::on(&self.engine, "named-leaf-undo");
        execute_rollback(
            &self.vm,
            writer,
            &plan.original,
            &plan.inverse_op_id,
            Initiator::User,
            reversal.runner(),
        )
        .await
    }
}

fn in_memory(name: &str) -> Arc<InMemoryVolume> {
    Arc::new(InMemoryVolume::new(name).with_space_info(1_000_000, 900_000))
}

async fn read(volume: &InMemoryVolume, path: &str) -> Vec<u8> {
    let mut reader = volume
        .open_read_stream(AppPath::from_normalized(path).expect("a canonical path"))
        .await
        .unwrap_or_else(|e| panic!("open {path}: {e:?}"));
    reader
        .next_chunk()
        .await
        .expect("a chunk")
        .unwrap_or_else(|e| panic!("read {path}: {e:?}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undoing_a_named_local_copy_deletes_exactly_the_named_copy() {
    for folder in [false, true] {
        let rig = UndoRig::new();
        rig.vm.register("root", crate::host::testing::scratch_disk_root());
        let work = tempfile::tempdir().expect("work dir");
        let source = work.path().join("src/original");
        let landed = work.path().join("dst/backup");
        let bystander = work.path().join("dst/original");
        if folder {
            std::fs::create_dir_all(source.join("empty")).expect("source folder");
            std::fs::write(source.join("child.txt"), b"payload").expect("child");
        } else {
            std::fs::create_dir_all(work.path().join("src")).expect("src");
            std::fs::write(&source, b"payload").expect("source file");
        }
        std::fs::create_dir_all(work.path().join("dst")).expect("dst");
        std::fs::write(&bystander, b"payload").expect("bystander");

        let op_id = if folder {
            "named-local-folder-copy"
        } else {
            "named-local-copy"
        };
        let state = Arc::new(rig.engine.new_operation_state(Duration::ZERO));
        journal::open_local_op(state.scope(), op_id, OpKind::Copy, Initiator::User, 1, Some("root"));
        let config = WriteOperationConfig {
            leaves: crate::types::TransferLeaves::single("backup"),
            ..Default::default()
        };
        let result = copy_files_with_progress_inner(
            &CollectorEventSink::new(),
            op_id,
            &state,
            std::slice::from_ref(&source),
            &work.path().join("dst"),
            &config,
        );
        journal::finalize_op(
            state.scope(),
            OpKind::Copy,
            journal::execution_status_from_error(result.as_ref().err()),
        );
        result.expect("the named copy ran");
        assert!(landed.exists(), "the copy landed under the requested leaf");

        let report = rig.undo(op_id).await;
        assert_eq!(report.skipped, 0, "nothing skipped, got {report:?}");
        assert!(!landed.exists(), "undo removed the named copy");
        assert_eq!(std::fs::read(&bystander).expect("bystander"), b"payload");
        assert!(source.exists(), "undo of a copy never touches its source");
    }
}

/// A copy naming each item (a renamed file, a renamed folder, one keeping its
/// name) undoes as one operation, deleting exactly the copies it made, under
/// whichever name each landed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undoing_a_copy_naming_each_item_deletes_exactly_its_copies() {
    let rig = UndoRig::new();
    rig.vm.register("root", crate::host::testing::scratch_disk_root());
    let work = tempfile::tempdir().expect("work dir");
    let (src, dst) = (work.path().join("src"), work.path().join("dst"));
    std::fs::create_dir_all(src.join("scans/empty")).expect("source folder");
    std::fs::write(src.join("scans/page.png"), b"page").expect("child");
    std::fs::write(src.join("Invoice-X.pdf"), b"invoice").expect("file");
    std::fs::write(src.join("receipt.pdf"), b"receipt").expect("kept");
    std::fs::create_dir_all(&dst).expect("dst");
    std::fs::write(dst.join("Invoice-X.pdf"), b"bystander").expect("bystander");
    let sources = [src.join("Invoice-X.pdf"), src.join("scans"), src.join("receipt.pdf")];

    let op_id = "per-source-local-copy";
    let state = Arc::new(rig.engine.new_operation_state(Duration::ZERO));
    journal::open_local_op(state.scope(), op_id, OpKind::Copy, Initiator::User, 3, Some("root"));
    let config = WriteOperationConfig {
        leaves: crate::types::TransferLeaves::per_source([
            (sources[0].clone(), "2026-09-27 invoice.pdf".to_string()),
            (sources[1].clone(), "2026-09 scans".to_string()),
        ]),
        ..Default::default()
    };
    let result = copy_files_with_progress_inner(&CollectorEventSink::new(), op_id, &state, &sources, &dst, &config);
    journal::finalize_op(
        state.scope(),
        OpKind::Copy,
        journal::execution_status_from_error(result.as_ref().err()),
    );
    result.expect("the named copy ran");
    assert_eq!(
        std::fs::read(dst.join("2026-09 scans/page.png")).expect("named folder"),
        b"page"
    );

    let report = rig.undo(op_id).await;
    assert_eq!(report.skipped, 0, "nothing skipped, got {report:?}");
    let mut left: Vec<String> = std::fs::read_dir(&dst)
        .expect("dst")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, vec!["Invoice-X.pdf"], "only the bystander stays");
    assert_eq!(
        std::fs::read(dst.join("Invoice-X.pdf")).expect("bystander"),
        b"bystander"
    );
    for source in &sources {
        assert!(source.exists(), "undo of a copy never touches its source");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undoing_a_named_volume_copy_deletes_exactly_the_named_copy() {
    let rig = UndoRig::new();
    let source = in_memory("Src");
    let dest = in_memory("Dst");
    rig.vm.register("smb-src", Arc::clone(&source) as Arc<dyn Volume>);
    rig.vm.register("smb-dst", Arc::clone(&dest) as Arc<dyn Volume>);
    source
        .create_file(app_path!("/original.txt"), b"payload")
        .await
        .expect("source");
    dest.create_file(app_path!("/original.txt"), b"payload")
        .await
        .expect("bystander");

    let op_id = "named-volume-copy";
    let state = Arc::new(
        rig.engine
            .new_operation_state(Duration::ZERO)
            .with_journal_volumes("smb-src".into(), "smb-dst".into()),
    );
    journal::open_volume_op(
        state.scope(),
        op_id,
        OpKind::Copy,
        Initiator::User,
        "smb-src",
        Some("smb-dst"),
        1,
    );
    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        op_id,
        &state,
        Arc::clone(&source) as Arc<dyn Volume>,
        &[PathBuf::from("/original.txt")],
        Arc::clone(&dest) as Arc<dyn Volume>,
        Path::new("/"),
        &VolumeCopyConfig {
            leaves: crate::types::TransferLeaves::single("backup.txt"),
            progress_interval_ms: 0,
            ..VolumeCopyConfig::default()
        },
    )
    .await
    .expect("the named volume copy ran");
    journal::finalize_op(state.scope(), OpKind::Copy, ExecutionStatus::Done);
    assert_eq!(read(&dest, "/backup.txt").await, b"payload");

    let report = rig.undo(op_id).await;
    assert_eq!(report.skipped, 0, "nothing skipped, got {report:?}");
    assert!(
        !dest.exists(app_path!("/backup.txt")).await,
        "undo removed the named copy"
    );
    assert_eq!(read(&dest, "/original.txt").await, b"payload", "the bystander stays");
    assert!(source.exists(app_path!("/original.txt")).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undoing_a_named_volume_move_restores_the_original_name() {
    for same_volume in [false, true] {
        let rig = UndoRig::new();
        let source = in_memory("Src");
        let dest = if same_volume {
            Arc::clone(&source)
        } else {
            in_memory("Dst")
        };
        let (source_id, dest_id) = if same_volume {
            ("smb-one", "smb-one")
        } else {
            ("smb-src", "smb-dst")
        };
        rig.vm.register(source_id, Arc::clone(&source) as Arc<dyn Volume>);
        rig.vm.register(dest_id, Arc::clone(&dest) as Arc<dyn Volume>);
        source
            .create_file(app_path!("/original.txt"), b"payload")
            .await
            .expect("source");
        dest.create_directory(app_path!("/dest")).await.expect("dest folder");
        dest.create_file(app_path!("/dest/original.txt"), b"bystander")
            .await
            .expect("bystander");

        let op_id = if same_volume {
            "named-same-volume-move"
        } else {
            "named-cross-volume-move"
        };
        let state = Arc::new(
            rig.engine
                .new_operation_state(Duration::ZERO)
                .with_journal_volumes(source_id.into(), dest_id.into()),
        );
        journal::open_volume_op(
            state.scope(),
            op_id,
            OpKind::Move,
            Initiator::User,
            source_id,
            Some(dest_id),
            1,
        );
        let config = VolumeCopyConfig {
            leaves: crate::types::TransferLeaves::single("renamed.txt"),
            progress_interval_ms: 0,
            ..VolumeCopyConfig::default()
        };
        let sources = [PathBuf::from("/original.txt")];
        let events = Arc::new(CollectorEventSink::new());
        if same_volume {
            move_within_same_volume_with_progress(
                events,
                op_id,
                &state,
                Arc::clone(&source) as Arc<dyn Volume>,
                &sources,
                Path::new("/dest"),
                &config,
            )
            .await
            .expect("the named same-volume move ran");
        } else {
            move_volumes_with_progress(
                events,
                op_id,
                &state,
                Arc::clone(&source) as Arc<dyn Volume>,
                &sources,
                Arc::clone(&dest) as Arc<dyn Volume>,
                Path::new("/dest"),
                &config,
            )
            .await
            .expect("the named cross-volume move ran");
        }
        journal::finalize_op(state.scope(), OpKind::Move, ExecutionStatus::Done);
        assert_eq!(read(&dest, "/dest/renamed.txt").await, b"payload");
        assert!(!source.exists(app_path!("/original.txt")).await);

        let report = rig.undo(op_id).await;
        assert_eq!(report.skipped, 0, "nothing skipped, got {report:?}");
        assert_eq!(
            read(&source, "/original.txt").await,
            b"payload",
            "the original name is back"
        );
        assert!(
            !dest.exists(app_path!("/dest/renamed.txt")).await,
            "the named leaf is gone"
        );
        assert_eq!(
            read(&dest, "/dest/original.txt").await,
            b"bystander",
            "the bystander stays"
        );
    }
}
