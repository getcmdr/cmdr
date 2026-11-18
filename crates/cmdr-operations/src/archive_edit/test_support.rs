//! Shared helpers and common re-exports for the archive-edit test modules. Each
//! `*_tests.rs` file does `use super::*;` (the module's public routes) plus
//! `use super::test_support::*;` (these helpers and the common types), so the test
//! bodies read the same as when they lived in one file.

use cmdr_fs::app_path::AppPath;
use uuid::Uuid;

// Common re-exports the test bodies reference by bare name (they also serve
// test_support's own helpers below).
pub(super) use std::io::{Read, Write};
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::sync::{Arc, Condvar, Mutex};
pub(super) use std::time::Duration;

pub(super) use super::super::OperationEventSink;
pub(super) use super::super::event_sinks::CollectorEventSink;
pub(super) use super::super::manager::OperationSummaryText;
pub(super) use super::super::testing::{TestOperationLog, TestOperations};
pub(super) use super::super::types::{
    ConflictId, ConflictInfo, ConflictResolution, DryRunResult, ScanProgressEvent, WriteCancelledEvent,
    WriteCompleteEvent, WriteConflictResolvedEvent, WriteErrorEvent, WriteOperationError, WriteOperationPhase,
    WriteOperationType, WriteProgressEvent, WriteSettledEvent, WriteSourceItemDoneEvent,
};
pub(super) use cmdr_archive::mutator::Changeset;
pub(super) use cmdr_fs::ignore_poison::IgnorePoison;
pub(super) use cmdr_fs::testing::wait_until_async;
pub(super) use cmdr_fs::volume::Volume;
pub(super) use cmdr_operation_log::rollback::VolumeLookup;
pub(super) use zip::write::SimpleFileOptions;
pub(super) use zip::{ZipArchive, ZipWriter};

/// Collects every event and blocks the operation exactly when `phase` is
/// emitted. Tests can inspect the in-flight boundary, then call [`Self::release`]
/// to let the operation continue. This is a condition gate, not a timing sleep.
pub(super) struct PhaseGateSink {
    pub inner: CollectorEventSink,
    phase: WriteOperationPhase,
    remaining_matches: Mutex<usize>,
    entered: Mutex<bool>,
    released: Mutex<bool>,
    gate: Condvar,
}

impl PhaseGateSink {
    pub(super) fn new(phase: WriteOperationPhase) -> Arc<Self> {
        Self::new_nth(phase, 1)
    }

    pub(super) fn new_nth(phase: WriteOperationPhase, occurrence: usize) -> Arc<Self> {
        assert!(occurrence > 0, "a phase gate occurrence is one-based");
        Arc::new(Self {
            inner: CollectorEventSink::new(),
            phase,
            remaining_matches: Mutex::new(occurrence),
            entered: Mutex::new(false),
            released: Mutex::new(false),
            gate: Condvar::new(),
        })
    }

    pub(super) fn entered(&self) -> bool {
        *self.entered.lock_ignore_poison()
    }

    pub(super) fn release(&self) {
        *self.released.lock_ignore_poison() = true;
        self.gate.notify_all();
    }
}

impl OperationEventSink for PhaseGateSink {
    fn emit_progress(&self, event: WriteProgressEvent) {
        let should_gate = event.phase == self.phase && {
            let mut remaining = self.remaining_matches.lock_ignore_poison();
            if *remaining == 0 {
                false
            } else {
                *remaining -= 1;
                *remaining == 0
            }
        };
        self.inner.emit_progress(event);
        if should_gate {
            *self.entered.lock_ignore_poison() = true;
            let mut released = self.released.lock_ignore_poison();
            while !*released {
                released = self
                    .gate
                    .wait(released)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        }
    }

    fn emit_complete(&self, event: WriteCompleteEvent) {
        self.inner.emit_complete(event);
    }

    fn emit_cancelled(&self, event: WriteCancelledEvent) {
        self.inner.emit_cancelled(event);
    }

    fn emit_error(&self, event: WriteErrorEvent) {
        self.inner.emit_error(event);
    }

    fn emit_conflict(&self, event: crate::ArmedConflict) {
        self.inner.emit_conflict(event);
    }

    fn emit_conflict_resolved(&self, event: WriteConflictResolvedEvent) {
        self.inner.emit_conflict_resolved(event);
    }

    fn emit_source_item_done(&self, event: WriteSourceItemDoneEvent) {
        self.inner.emit_source_item_done(event);
    }

    fn emit_scan_progress(&self, event: ScanProgressEvent) {
        self.inner.emit_scan_progress(event);
    }

    fn emit_scan_conflict(&self, conflict: ConflictInfo) {
        self.inner.emit_scan_conflict(conflict);
    }

    fn emit_dry_run_complete(&self, result: DryRunResult) {
        self.inner.emit_dry_run_complete(result);
    }

    fn emit_settled(&self, event: WriteSettledEvent) {
        self.inner.emit_settled(event);
    }
}

/// Builds a one-entry zip at `path`.
pub(super) fn write_simple_zip(path: &Path, entry: &str, content: &[u8]) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file(entry, SimpleFileOptions::default())
        .expect("start entry");
    writer.write_all(content).expect("write entry");
    writer.finish().expect("finish zip");
}

/// Builds a multi-entry zip at `path`.
pub(super) fn write_multi_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut writer = ZipWriter::new(file);
    for (name, content) in entries {
        writer.start_file(*name, SimpleFileOptions::default()).expect("start");
        writer.write_all(content).expect("write");
    }
    writer.finish().expect("finish zip");
}

/// Reads one entry's decompressed bytes back, or `None` if absent.
pub(super) fn read_entry(path: &Path, name: &str) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name(name).ok()?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// A unique id giving one archive-edit test op its OWN operation-manager lane,
/// instead of a lane shared with other concurrently-running tests. Used as a
/// LOCAL parent-drive id (an unregistered id resolves to a local in-place edit,
/// exactly like the production `"root"` disk, so its fallback lane is the id
/// itself), or as an explicit `with_lane_key` on a test volume.
///
/// This is for test ISOLATION and parallel speed, mirroring the manager's own
/// `tests.rs` ("unique operation ids + lane keys"): the operation manager is a
/// process-global singleton, so a shared global lane (like `"root"`, or an
/// `InMemoryVolume`'s default root-`/` lane) serializes otherwise-unrelated tests
/// and couples their timing. Keep new local tests on this, NOT `"root"`. It is
/// NOT the orphan-safety mechanism — that's the manager spawning admitted ops on
/// the app runtime (`lifecycle/manager.rs` § the admission pass), which holds regardless of
/// lanes.
pub(super) fn unique_lane_id() -> String {
    format!("test-lane-{}", Uuid::new_v4())
}

/// A real zip as in-memory bytes, for seeding a remote parent's store.
pub(super) fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        for (name, content) in entries {
            writer.start_file(*name, SimpleFileOptions::default()).expect("start");
            writer.write_all(content).expect("write");
        }
        writer.finish().expect("finish");
    }
    cursor.into_inner()
}

/// A NON-local `InMemoryVolume` (the remote-parent stand-in) holding a zip at
/// `archive_path`, under a unique volume id, registered on `engine`.
pub(super) async fn register_remote_zip(
    engine: &TestOperations,
    archive_path: &Path,
    entries: &[(&str, &[u8])],
) -> (String, Arc<cmdr_fs::volume::InMemoryVolume>) {
    let (id, parent) = remote_zip(archive_path, entries).await;
    engine.volumes().register(&id, Arc::clone(&parent) as Arc<dyn Volume>);
    (id, parent)
}

/// [`register_remote_zip`]'s volume and id, before it registers them.
pub(super) async fn remote_zip(
    archive_path: &Path,
    entries: &[(&str, &[u8])],
) -> (String, Arc<cmdr_fs::volume::InMemoryVolume>) {
    use cmdr_fs::volume::InMemoryVolume;
    let id = format!("remote-test-{}", Uuid::new_v4());
    // The lane key is the unique volume id so each remote-parent op gets its own
    // operation-manager lane. An `InMemoryVolume` otherwise defaults its lane to
    // its root `/`, shared across every instance — and a copy-into reserves the
    // parent's lane, so two such ops would serialize otherwise-unrelated tests
    // (see `unique_lane_id` for the isolation rationale).
    let parent = InMemoryVolume::new("Remote").with_lane_key(id.clone());
    if let Some(dir) = archive_path.parent() {
        parent
            .create_directory(&AppPath::legacy_unchecked(dir))
            .await
            .expect("seed parent dir");
    }
    parent
        .create_file(&AppPath::legacy_unchecked(archive_path), &zip_bytes(entries))
        .await
        .expect("seed remote zip");
    (id, Arc::new(parent))
}

/// Registers a NON-local `InMemoryVolume` (a remote SOURCE stand-in — MTP / SMB)
/// pre-populated with `files` (each `(relative_path, bytes)`), creating every
/// parent directory the paths imply, registered on `engine`. Returns the volume
/// id and the handle.
pub(super) async fn register_remote_source(
    engine: &TestOperations,
    files: &[(&str, &[u8])],
) -> (String, Arc<cmdr_fs::volume::InMemoryVolume>) {
    use cmdr_fs::volume::InMemoryVolume;
    let source = InMemoryVolume::new("RemoteSource");
    for (rel, bytes) in files {
        let path = PathBuf::from("/").join(rel);
        if let Some(parent) = path.parent() {
            let mut acc = PathBuf::from("/");
            for comp in parent.strip_prefix("/").unwrap_or(parent).components() {
                acc = acc.join(comp);
                // Ignore "already exists" for shared ancestors across files.
                let _ = source.create_directory(&AppPath::legacy_unchecked(&acc)).await;
            }
        }
        source
            .create_file(&AppPath::legacy_unchecked(&path), bytes)
            .await
            .expect("seed remote source file");
    }
    let source = Arc::new(source);
    let id = format!("remote-source-{}", Uuid::new_v4());
    engine.volumes().register(&id, Arc::clone(&source) as Arc<dyn Volume>);
    (id, source)
}

/// A remote edit's upload of the rewritten archive speaks for itself: a
/// `Transferring` axis over the uploaded archive's bytes, then indeterminate
/// `FinishingTransfer` for close and swap, never a silent tail after 100%.
pub(super) async fn assert_upload_reported_honestly(
    events: &CollectorEventSink,
    parent: &dyn Volume,
    archive_path: &Path,
) {
    let uploaded_size = parent
        .get_metadata(&AppPath::legacy_unchecked(archive_path))
        .await
        .expect("stat the published archive")
        .size
        .expect("the archive has a size");
    let progress = events.progress.lock_ignore_poison();
    let upload = progress
        .iter()
        .filter(|event| event.phase == WriteOperationPhase::Transferring)
        .collect::<Vec<_>>();
    assert!(
        !upload.is_empty(),
        "the upload of the rewritten archive must report its own bytes"
    );
    assert!(
        upload.iter().all(|event| event.bytes_total == uploaded_size),
        "the transfer axis is the rewritten archive's size"
    );
    assert!(
        progress
            .iter()
            .any(|event| event.phase == WriteOperationPhase::FinishingTransfer),
        "close and swap read as indeterminate finishing work"
    );
}

/// Streams the archive back out of a (remote) parent and returns one entry's bytes.
pub(super) async fn read_remote_entry(parent: &dyn Volume, archive_path: &Path, name: &str) -> Option<Vec<u8>> {
    let mut stream = parent
        .open_read_stream(&AppPath::legacy_unchecked(archive_path))
        .await
        .ok()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        bytes.extend_from_slice(&chunk.ok()?);
    }
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut entry = archive.by_name(name).ok()?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).ok()?;
    Some(buf)
}
