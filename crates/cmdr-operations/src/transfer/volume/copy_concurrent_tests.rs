//! Concurrency tests for `copy_volumes_with_progress`, split out of
//! `volume/copy_tests/`. These exercise the concurrent batch path:
//! all-succeed, abort-on-first-error, and cancellation mid-batch. The
//! `PoisonedReadVolume` double injects a read failure, and the local
//! `CancelOnProgressSink` flips the operation's intent mid-flight.
//!
//! Shared fixtures `make_state` / `make_volumes` live in `volume/copy_tests/mod.rs`
//! (`super::tests`).

use super::super::super::conflict_responder_test_support::await_prompted_clash;
use super::tests::{make_state, make_volumes};
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::state::ConflictResolutionResponse;
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, WriteConflictResolvedEvent, WriteErrorEvent, WriteSourceItemDoneEvent};
use cmdr_fs::app_path;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{CopyScanResult, InMemoryVolume, ListingProgress, SpaceInfo, VolumeReadStream};

// ── Phase 4.2 concurrency tests ──────────────────────────────────
//
// Exercise the concurrent path in `copy_volumes_with_progress`.
// `InMemoryVolume` returns `max_concurrent_ops() = 32`, so batches of
// 3+ files automatically take the concurrent branch (clamped to 32).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_copy_50_files_all_succeed() {
    let engine = TestOperations::new();
    let (source, dest) = make_volumes();

    // 50 small files, well over the threshold=3 and concurrency=32.
    for i in 0..50 {
        let name = format!("/file_{:02}.bin", i);
        source
            .create_file(&AppPath::legacy_unchecked(Path::new(&name)), &vec![i as u8; 1024])
            .await
            .unwrap();
    }

    let events = Arc::new(CollectorEventSink::new());
    let state = make_state(&engine);
    let config = VolumeCopyConfig {
        progress_interval_ms: 0, // Emit on every progress call
        ..VolumeCopyConfig::default()
    };

    let paths: Vec<PathBuf> = (0..50).map(|i| PathBuf::from(format!("/file_{:02}.bin", i))).collect();
    let result = copy_volumes_with_progress(
        events.clone(),
        "test-op-concurrent-50",
        &state,
        Arc::clone(&source),
        &paths,
        Arc::clone(&dest),
        Path::new("/"),
        &config,
    )
    .await;
    assert!(result.is_ok(), "expected success, got {:?}", result);

    // All 50 files landed at destination with the right content.
    for i in 0..50 {
        let name = format!("/file_{:02}.bin", i);
        let mut stream = dest
            .open_read_stream(&AppPath::legacy_unchecked(Path::new(&name)))
            .await
            .unwrap();
        let mut collected = Vec::new();
        while let Some(Ok(chunk)) = stream.next_chunk().await {
            collected.extend_from_slice(&chunk);
        }
        assert_eq!(collected, vec![i as u8; 1024], "wrong content for {}", name);
    }

    // Progress events were emitted (throttled, but >= 1 under concurrency).
    let progress = events.progress.lock().unwrap();
    assert!(
        !progress.is_empty(),
        "expected at least one progress event under concurrency"
    );

    // Completion event with correct totals.
    let complete = events.complete.lock().unwrap();
    assert_eq!(complete.len(), 1);
    assert_eq!(complete[0].files_processed, 50);
    assert_eq!(complete[0].bytes_processed, 50 * 1024);
}

/// Volume wrapper that delegates everything to an inner `InMemoryVolume`
/// except for a single poisoned filename, which returns an I/O error on
/// read. Used to exercise abort-on-first-error under concurrency.
struct PoisonedReadVolume {
    inner: Arc<InMemoryVolume>,
    poisoned_file: String,
}

impl Volume for PoisonedReadVolume {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn root(&self) -> &AppPath {
        self.inner.root()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        self.inner.list_directory(path, on_progress)
    }
    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        self.inner.get_metadata(path)
    }
    fn exists<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        self.inner.exists(path)
    }
    fn is_directory<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        self.inner.is_directory(path)
    }
    fn supports_export(&self) -> bool {
        true
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    fn max_concurrent_ops(&self) -> usize {
        32
    }
    fn scan_for_copy<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<CopyScanResult, VolumeError>> + Send + 'a>> {
        self.inner.scan_for_copy(path)
    }
    fn get_space_info<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        self.inner.get_space_info()
    }
    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        let name = self.poisoned_file.clone();
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let path_app = path;
            let path_std = path.as_std_legacy();
            let path: &Path = &path_std;
            if path.to_string_lossy() == name {
                return Err(VolumeError::IoError {
                    message: "Injected read failure".into(),
                    raw_os_error: Some(5), // EIO
                });
            }
            inner.open_read_stream(path_app).await
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_copy_aborts_on_first_error() {
    let engine = TestOperations::new();
    let inner_source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    for i in 0..20 {
        let name = format!("/file_{:02}.bin", i);
        inner_source
            .create_file(&AppPath::legacy_unchecked(Path::new(&name)), &vec![0xAB; 1024])
            .await
            .unwrap();
    }
    // File 05 will fail when read.
    let source: Arc<dyn Volume> = Arc::new(PoisonedReadVolume {
        inner: Arc::clone(&inner_source),
        poisoned_file: "/file_05.bin".to_string(),
    });
    let dest: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));

    let events = Arc::new(CollectorEventSink::new());
    let state = make_state(&engine);
    let config = VolumeCopyConfig::default();

    let paths: Vec<PathBuf> = (0..20).map(|i| PathBuf::from(format!("/file_{:02}.bin", i))).collect();
    let result = copy_volumes_with_progress(
        events.clone(),
        "test-op-concurrent-err",
        &state,
        Arc::clone(&source),
        &paths,
        Arc::clone(&dest),
        Path::new("/"),
        &config,
    )
    .await;

    // Must return an IoError (the injected one). The in-flight tasks drop
    // cleanly and the outer loop returns the mapped error.
    assert!(matches!(
        result,
        Err(WriteFailure {
            error: WriteOperationError::IoError { .. },
            ..
        })
    ));

    // Not all 20 files should be at the dest (some were still in flight
    // or not yet started when the abort fired). The poisoned file itself
    // cannot have landed.
    assert!(!dest.exists(app_path!("/file_05.bin")).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_copy_cancellation_mid_batch() {
    let engine = TestOperations::new();
    // Custom event sink that flips the intent to Stopped after a few
    // progress events land. Deterministic: doesn't rely on timing.
    struct CancelOnProgressSink {
        inner: CollectorEventSink,
        intent: crate::state::IntentCell,
        cancel_after_events: usize,
        events_seen: AtomicUsize,
    }
    impl OperationEventSink for CancelOnProgressSink {
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
    // 20 large-ish files so the batch stays in flight long enough
    // for the cancel to land while tasks are running.
    for i in 0..20 {
        let name = format!("/big_{:02}.bin", i);
        source
            .create_file(&AppPath::legacy_unchecked(Path::new(&name)), &vec![i as u8; 200_000])
            .await
            .unwrap();
    }

    let state = make_state(&engine);
    let events = Arc::new(CancelOnProgressSink {
        inner: CollectorEventSink::new(),
        intent: state.intent().clone(),
        cancel_after_events: 2,
        events_seen: AtomicUsize::new(0),
    });
    let config = VolumeCopyConfig {
        progress_interval_ms: 0, // Emit on every chunk so we can trigger early.
        ..VolumeCopyConfig::default()
    };

    let paths: Vec<PathBuf> = (0..20).map(|i| PathBuf::from(format!("/big_{:02}.bin", i))).collect();
    let result = copy_volumes_with_progress(
        events.clone(),
        "test-op-concurrent-cancel",
        &state,
        Arc::clone(&source),
        &paths,
        Arc::clone(&dest),
        Path::new("/"),
        &config,
    )
    .await;

    // Either Cancelled (pure cancel branch) or IoError (if a task's
    // progress callback returned Break). Both are valid cancellation
    // shapes, matching the sequential test `test_multi_file_copy_cancel_mid_flight`.
    assert!(
        matches!(
            result,
            Err(WriteFailure {
                error: WriteOperationError::Cancelled { .. },
                ..
            }) | Err(WriteFailure {
                error: WriteOperationError::IoError { .. },
                ..
            })
        ),
        "expected Cancelled or IoError, got {:?}",
        result
    );

    // Intent was flipped to Stopped by the sink; confirm we observed it.
    assert_eq!(state.intent().load(), OperationIntent::Stopped);

    // Less than all 20 landed (cancellation worked somewhere).
    let mut total = 0;
    for i in 0..20 {
        if dest
            .exists(&AppPath::legacy_unchecked(Path::new(&format!("/big_{:02}.bin", i))))
            .await
        {
            total += 1;
        }
    }
    assert!(total < 20, "expected fewer than 20 files at dest, got {}", total);
}

/// While the driver is parked on a top-level Stop prompt, the source it asks
/// about and every source after it stay put, while the source already in the
/// window runs to the end.
///
/// That's the point of resolving a conflict on the driver, before the task is
/// pushed. The dispatch mutex would still serialize the prompts if resolution
/// moved into the tasks, and the "…all" latch would still collapse the queued
/// ones — so prompt COUNTS can't tell those designs apart. What changes is what
/// the rest of the batch does while a person is deciding: here it waits its
/// turn; the alternative copies later sources, and the prompts arrive out of
/// order. The source already in the window lands AND is recorded while the
/// prompt is up (`ConcurrentDriver::prepare_while_draining`), so its
/// `write-source-item-done` doesn't wait on the person's answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_top_level_conflict_prompt_holds_back_its_source_and_every_later_one() {
    let engine = TestOperations::new();
    let (source, dest) = make_volumes();

    // One source before the clash (already in the window, so it finishes) and
    // one after it (must not even be prepared). The prefixes keep that order:
    // the driver starts sources in tree order, which sorts by name.
    source.create_file(app_path!("/a-first.bin"), b"ONE").await.unwrap();
    source.create_file(app_path!("/b-clash.txt"), b"SRC").await.unwrap();
    source.create_file(app_path!("/c-later.bin"), b"TWO").await.unwrap();
    dest.create_file(app_path!("/b-clash.txt"), b"DEST").await.unwrap();

    let state = make_state(&engine);
    let events = Arc::new(CollectorEventSink::new());
    let config = VolumeCopyConfig {
        conflict_resolution: ConflictResolution::Stop,
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };

    // The copy runs on its own task, so the driver is genuinely free to keep
    // working while the prompt is outstanding — which is exactly what must not
    // happen. Everything it borrows is moved in.
    let sources = vec![
        PathBuf::from("/a-first.bin"),
        PathBuf::from("/b-clash.txt"),
        PathBuf::from("/c-later.bin"),
    ];
    let copy = tokio::spawn({
        let events = Arc::clone(&events);
        let state = Arc::clone(&state);
        let source = Arc::clone(&source);
        let dest = Arc::clone(&dest);
        async move {
            copy_volumes_with_progress(
                events,
                "op-prompt-holds-the-batch",
                &state,
                source,
                &sources,
                dest,
                Path::new("/"),
                &config,
            )
            .await
        }
    });

    let clash = await_prompted_clash(&events).await;
    cmdr_fs::testing::wait_until_async(
        Duration::from_secs(5),
        "the source already in the window to land",
        || {
            events
                .source_items_done
                .lock()
                .unwrap()
                .iter()
                .any(|done| done.source_path == "/a-first.bin")
        },
    )
    .await;
    // A negative assertion needs a window to be worth anything: give the driver
    // real time to do the thing it must not do. A batch of three-byte files
    // copies in microseconds, so this is generous.
    // allowed-test-sleep: negative assertion over a window — the claim is that
    // nothing after the clash moves while the prompt is up, which needs
    // elapsed time, not a condition to poll for.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !dest.exists(app_path!("/c-later.bin")).await,
        "a source after the clash must not be copied while the user is still being asked"
    );
    assert_eq!(
        dest.get_metadata(app_path!("/b-clash.txt")).await.unwrap().size,
        Some(4),
        "the destination the user is being asked about must stay untouched"
    );
    let _ = state.conflict_slot.answer(
        clash,
        ConflictResolutionResponse {
            resolution: ConflictResolution::Overwrite,
            apply_to_all: false,
        },
    );
    let result = copy.await.unwrap();

    assert!(result.is_ok(), "expected Ok, got {result:?}");
    // And once the answer lands, the batch runs to completion normally.
    assert!(dest.exists(app_path!("/a-first.bin")).await);
    assert!(dest.exists(app_path!("/c-later.bin")).await);
}

/// Progress from several top-level tasks streaming in parallel never steps
/// backwards, on either bar.
///
/// The tasks are spawned, so they report from different threads at once. Each
/// total is worked out under the ledger's lock, but an event built from an older
/// total could still reach the sink after a newer one, and the dialog's bars,
/// its ETA, and the stall watchdog would all read the reversal as lost work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_from_parallel_tasks_never_steps_backwards() {
    let engine = TestOperations::new();
    let (source, dest) = make_volumes();
    let paths: Vec<PathBuf> = (0..16).map(|i| PathBuf::from(format!("/par_{i:02}.bin"))).collect();
    for (i, path) in paths.iter().enumerate() {
        source
            .create_file(&AppPath::legacy_unchecked(path), &vec![i as u8; 256 * 1024])
            .await
            .unwrap();
    }

    let events = Arc::new(CollectorEventSink::new());
    let state = make_state(&engine);
    let config = VolumeCopyConfig {
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };
    copy_volumes_with_progress(
        events.clone(),
        "op-parallel-progress",
        &state,
        source,
        &paths,
        dest,
        Path::new("/"),
        &config,
    )
    .await
    .expect("the batch copies");

    let progress = events.progress.lock().unwrap();
    let copying: Vec<(usize, u64)> = progress
        .iter()
        .filter(|p| p.phase == WriteOperationPhase::Copying)
        .map(|p| (p.files_done, p.bytes_done))
        .collect();
    for pair in copying.windows(2) {
        assert!(
            pair[0].0 <= pair[1].0 && pair[0].1 <= pair[1].1,
            "(files_done, bytes_done) stepped back from {:?} to {:?}",
            pair[0],
            pair[1]
        );
    }
}
