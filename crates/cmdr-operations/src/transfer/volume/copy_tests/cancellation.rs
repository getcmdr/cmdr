//! Cancelling a multi-file copy, before it starts and mid-flight.

use super::*;
use crate::testing::TestOperations;
use cmdr_fs::app_path;
use std::path::Path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multi_file_copy_cancel_before_start() {
    let engine = TestOperations::new();
    let (source, dest) = make_volumes();

    source.create_file(app_path!("/a.txt"), b"alpha").await.unwrap();
    source.create_file(app_path!("/b.txt"), b"bravo").await.unwrap();

    let events = Arc::new(CollectorEventSink::new());
    let state = make_state(&engine);
    // Set Stopped BEFORE starting
    state.intent().set_for_test(OperationIntent::Stopped);
    let config = VolumeCopyConfig::default();

    let result = copy_volumes_with_progress(
        events.clone(),
        "test-op-pre-cancel",
        &state,
        Arc::clone(&source),
        &crate::testing::app_paths_of(&[PathBuf::from("/a.txt"), PathBuf::from("/b.txt")]),
        Arc::clone(&dest),
        &crate::testing::app_path_of(Path::new("/")),
        &config,
    )
    .await;

    assert!(matches!(
        result,
        Err(WriteFailure {
            error: WriteOperationError::Cancelled { .. },
            ..
        })
    ));
    // No files should have been copied
    assert!(!dest.exists(app_path!("/a.txt")).await);
    assert!(!dest.exists(app_path!("/b.txt")).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multi_file_copy_cancel_mid_flight() {
    let engine = TestOperations::new();
    // Custom event sink that flips intent to Stopped after a handful of
    // `Copying` progress events. Counting EVENTS (which fire per chunk, early in
    // the batch) rather than completed FILES makes the cancel land while tasks
    // are still mid-stream — robust regardless of scheduler interleaving. The
    // concurrent streaming path parks/checks between chunks, so the in-flight
    // tasks break at their next checkpoint and not all sources finish.
    struct CancelAfterNSink {
        inner: CollectorEventSink,
        intent: crate::state::IntentCell,
        cancel_after_events: usize,
        events_seen: AtomicUsize,
    }

    impl OperationEventSink for CancelAfterNSink {
        fn emit_settled(&self, e: crate::types::WriteSettledEvent) {
            self.inner.emit_settled(e);
        }
        fn emit_progress(&self, event: WriteProgressEvent) {
            if event.phase == WriteOperationPhase::Copying
                && self.events_seen.fetch_add(1, Ordering::Relaxed) >= self.cancel_after_events
            {
                self.intent.set_for_test(OperationIntent::Stopped);
            }
            self.inner.emit_progress(event);
        }
        fn emit_complete(&self, e: WriteCompleteEvent) {
            self.inner.emit_complete(e);
        }
        fn emit_cancelled(&self, e: WriteCancelledEvent) {
            self.inner.emit_cancelled(e);
        }
        fn emit_error(&self, e: WriteErrorEvent) {
            self.inner.emit_error(e);
        }
        fn emit_conflict(&self, e: crate::ArmedConflict) {
            self.inner.emit_conflict(e);
        }
        fn emit_conflict_resolved(&self, e: WriteConflictResolvedEvent) {
            self.inner.emit_conflict_resolved(e);
        }
        fn emit_source_item_done(&self, _e: WriteSourceItemDoneEvent) {}
        fn emit_scan_progress(&self, _e: crate::types::ScanProgressEvent) {}
        fn emit_scan_conflict(&self, _c: crate::types::ConflictInfo) {}
        fn emit_dry_run_complete(&self, _r: crate::types::DryRunResult) {}
    }

    let (source, dest) = make_volumes();
    // Files large enough (many 64 KB chunks) that the three not-yet-complete
    // in-flight tasks reliably observe the cancel at a between-chunk checkpoint
    // before finishing. With tiny files the whole batch can complete inside one
    // scheduler turn, making "not all 5 land" a coin flip.
    for i in 1..=5 {
        source
            .create_file(
                &AppPath::legacy_unchecked(Path::new(&format!("/{}.bin", i))),
                &vec![0; 2_000_000],
            )
            .await
            .unwrap();
    }

    let state = make_state(&engine);
    let events = Arc::new(CancelAfterNSink {
        inner: CollectorEventSink::new(),
        intent: state.intent().clone(),
        cancel_after_events: 3,
        events_seen: AtomicUsize::new(0),
    });
    let config = VolumeCopyConfig {
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };

    let result = copy_volumes_with_progress(
        events.clone(),
        "test-op-cancel-mid",
        &state,
        Arc::clone(&source),
        &crate::testing::app_paths_of(&[
            PathBuf::from("/1.bin"),
            PathBuf::from("/2.bin"),
            PathBuf::from("/3.bin"),
            PathBuf::from("/4.bin"),
            PathBuf::from("/5.bin"),
        ]),
        Arc::clone(&dest),
        &crate::testing::app_path_of(Path::new("/")),
        &config,
    )
    .await;

    // Cancellation from write_from_stream's progress callback results in an IoError
    // (the VolumeError::IoError "Operation cancelled" maps to WriteOperationError::IoError).
    // The outer loop then detects the Stopped intent and returns Cancelled.
    assert!(result.is_err(), "expected error, got {:?}", result);

    // A mid-flight cancel leaves the batch partially done: fewer than all 5
    // sources land. Completion order under concurrency isn't deterministic, so
    // assert on the COUNT, not on specific names.
    let mut total = 0;
    for i in 1..=5 {
        if dest
            .exists(&AppPath::legacy_unchecked(Path::new(&format!("/{}.bin", i))))
            .await
        {
            total += 1;
        }
    }
    assert!(total < 5, "expected fewer than 5 files, got {}", total);

    // The cancel either emits a write-cancelled event (if the intent check fires
    // between files) or returns an error (if write_from_stream's progress callback
    // returned Break). Both are valid cancellation paths.
    let cancelled = events.inner.cancelled.lock().unwrap();
    let had_error = result.is_err();
    assert!(
        cancelled.len() == 1 || had_error,
        "expected either a cancelled event or an error"
    );
}
