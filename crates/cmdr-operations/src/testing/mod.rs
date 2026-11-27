//! An engine of a test's own ([`TestOperations`], `engine.rs`), and what test
//! cells outside the engine reach beyond its public list: the internals they
//! drive directly and the fixture they park operations with. A cell outside the
//! engine names the engine it drives, its own or the app's
//! (`operations_host::app_engine()`).
//!
//! Sized for the crate's `testing` feature: when the engine becomes a crate,
//! exactly this list widens from `cfg(test)` to `any(test, feature = "testing")`.
//! Gated items count apart from the public surface's ceilings, so this list is
//! the place a test-only reach goes, ❌ never the public list above it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::volume::Volume;
use cmdr_operation_log::types::Initiator;

use super::{
    BulkRenameRow, ConflictResolution, ExpectedSources, OperationEventSink, Operations, RenameStartError,
    VolumeCopyConfig, WriteOperationError, WriteOperationStartResult,
};
use cmdr_operation_log::types::OperationId;

// An engine a test builds for itself, over fakes it reads back, and a journal
// it reads rows back from.
mod engine;
pub use engine::{RegisteredOperation, TestOperationLog, TestOperations};

// Taking back what a test put in the user's real Trash.
mod trash_cleanup;
pub use trash_cleanup::TrashCleanup;
#[cfg(target_os = "macos")]
pub(crate) use trash_cleanup::note_trashed;

// A real queued operation, parked on the engine a cell names.
pub use super::test_support::QueuedOperationFixture;

// Driving one operation's state and sink by hand.
pub use super::event_sinks::CollectorEventSink;
pub use super::state::{IntentCell, OperationIntent, WriteOperationState};

/// A test's path as the typed starts take it. A native path (a scratch directory, a file
/// under one) crosses as the OS spells it, so `/tmp/x` on macOS and Linux and `C:/…/x` on
/// Windows, the spelling the app's command edge sends; app text a test held in a `PathBuf`
/// (a scheme path, a share's or phone's POSIX `/x`) reads as the wire reads it. A
/// VOLUME-RELATIVE path (a suite's `cmdr-test-…/a.txt` on a share, which the volume anchors
/// on its own root) has no app path without that root, so it goes on as a bridge value, its
/// bytes as held: the one crossing a suite's relative spelling takes (#1225).
#[track_caller]
pub fn app_path_of(path: &std::path::Path) -> cmdr_fs::app_path::AppPathBuf {
    use cmdr_fs::app_path::{AppPathBuf, PathFlavor};
    if let Ok(native) = AppPathBuf::from_native_unicode(path) {
        return native;
    }
    let text = path.to_str();
    match text.map(|text| AppPathBuf::from_wire(text).or_else(|_| AppPathBuf::parse(text, PathFlavor::Posix))) {
        Some(Ok(app)) => app,
        _ => crate::wire_paths::seam_path(path).into_owned(),
    }
}

/// The `std::path` value a start hands its bodies for `path` (`wire_paths::engine_path`, the
/// production crossing), for a cell that pins a host's whole door: its edge reading, then this.
pub fn engine_path_of(path: &cmdr_fs::app_path::AppPath) -> PathBuf {
    crate::wire_paths::engine_path(path)
}

/// A typed path as a rename's destination, through the one door that builds one
/// (`RenameTarget::read`), so a cell meets the dot-name refusal as every caller does.
/// Panics if the door refuses (a cell pinning a refusal calls `RenameTarget::read` itself).
#[track_caller]
pub fn rename_target(path: &cmdr_fs::app_path::AppPath) -> crate::RenameTarget {
    crate::RenameTarget::read(path.as_str(), |_| Ok(path.to_owned())).expect("a typed path's last name is a real name")
}

/// [`app_path_of`] over a start's sources.
#[track_caller]
pub fn app_paths_of<P: AsRef<std::path::Path>>(
    natives: impl IntoIterator<Item = P>,
) -> Vec<cmdr_fs::app_path::AppPathBuf> {
    let mut paths = Vec::new();
    for native in natives {
        paths.push(app_path_of(native.as_ref()));
    }
    paths
}

/// A fresh state on `engine`, unregistered: what a cell outside the engine drives
/// a volume driver with directly (a backend suite, on `operations_host::app_engine()`).
pub fn new_operation_state(engine: &Operations, progress_interval: Duration) -> WriteOperationState {
    engine.new_operation_state(progress_interval)
}

// What a suite asserts on: the settle a mutation reports, the binding a
// reviewed batch applies, the live tree's ancestry, and the index's leaf cap.
pub use super::journal_search::SEARCH_LEAF_CAP;
pub use super::mutation_reply::MutationSettledOutcome;
pub use super::source_binding::retain_bound_sources;
pub use super::transfer_tree::TransferTreeAncestor;

// Wire vocabulary only a payload a test builds by hand, or an assertion
// matches field by field, needs.
pub use super::types::{
    AppearedDuringMove, CancelRollback, LostMetadata, LostMetadataClass, MetadataLeftBehind, ProgressAxis,
    RollbackSupport, SourcesGoneNext, TransferWaitReason, TrashRefusalKind,
};

// The backend suites (`operations_host/backend_suites/`): the volume engine's
// drivers a cell runs on a live server's volume with its own state and sink,
// and the fault injector and safety oracle its scenarios wrap around them.
pub use super::transfer::volume::{
    FaultyOp, FaultyVolume, SafetySpec, assert_operation_was_safe, copy_volumes_with_progress,
    move_volumes_with_progress, move_within_same_volume_with_progress,
};
pub use crate::forward_volume_methods;

// The same suites' other direct drivers: a volume delete, a mkdir, a zip
// edit's remote steps, and an outage policy.
pub use super::archive_edit::{EditError, pull_apply_upload_swap};
pub use super::create::create_directory_core_on;

// The cells that drive the engine over the app's registry
// (`operations_host/registry_routing_tests.rs`): a zip edit's request, and the
// duplicate pre-check inside an archive.
pub use super::archive_edit::{ArchiveEditRequest, archive_inner_exists};
pub use super::manager::OperationSummaryText;

// The scan preview's log label, which the app's report redactor must clean
// (`operations_host/log_redaction_tests.rs`).
pub use super::delete::delete_volume_files_for_test;
pub use super::outage_slot::{CleanupPolicy, OutagePolicy};
pub use super::scan_watchdog::ScanTarget;

/// Seeds `engine`'s previews with an entry that fails the coherence canary, for a
/// backend cell pinning what an operation does when a half-empty preview reaches
/// it anyway. See `ScanPreviews::seed_incoherent_scan_result_for_test`.
pub fn seed_incoherent_scan_result(
    engine: &Operations,
    preview_id: String,
    sources: Vec<PathBuf>,
    file_count: usize,
    total_bytes: u64,
) {
    engine
        .stores()
        .previews()
        .seed_incoherent_scan_result_for_test(preview_id, sources, file_count, total_bytes);
}

// An undo's executor input, which the rollback cells build from a journaled
// operation.
pub use super::rollback::Reversal;

// Entry points production reaches only from inside the engine (a routed start,
// the queue's pause and resume), which an app-side cell drives directly. Here
// rather than on the handle, so the public surface promises none of them.

/// Starts `request` as a managed archive edit on `engine`: the driver a routed
/// create, rename, or transfer into a zip lands in.
pub async fn archive_edit_start(
    engine: &Operations,
    events: Arc<dyn OperationEventSink>,
    request: ArchiveEditRequest,
    progress_interval_ms: u64,
) -> Result<WriteOperationStartResult, WriteOperationError> {
    engine.archive_edit_start(events, request, progress_interval_ms).await
}

/// Routes an in-archive delete of `sources` on `engine`, as a routed delete does.
pub async fn route_archive_delete(
    engine: &Operations,
    events: Arc<dyn OperationEventSink>,
    sources: &[PathBuf],
    parent_volume_id: &str,
    progress_interval_ms: u64,
    preview_id: Option<String>,
) -> Result<WriteOperationStartResult, WriteOperationError> {
    engine
        .route_archive_delete(events, sources, parent_volume_id, progress_interval_ms, preview_id)
        .await
}

/// Starts a fresh compression on `engine`, as a routed compress does.
#[allow(
    clippy::too_many_arguments,
    reason = "forwards the start's own parameters one-to-one"
)]
pub async fn compress_start(
    engine: &Operations,
    events: Arc<dyn OperationEventSink>,
    source_volume: Arc<dyn Volume>,
    source_paths: Vec<PathBuf>,
    dest_zip_full_path: PathBuf,
    parent_volume_id: String,
    conflict: ConflictResolution,
    progress_interval_ms: u64,
    compression_level: Option<i64>,
    preview_id: Option<String>,
    initiator: Initiator,
) -> Result<WriteOperationStartResult, WriteOperationError> {
    engine
        .compress_start(
            events,
            source_volume,
            source_paths,
            dest_zip_full_path,
            parent_volume_id,
            conflict,
            progress_interval_ms,
            compression_level,
            preview_id,
            initiator,
        )
        .await
}

/// Starts a cross-volume copy on `engine`, as a routed copy does.
#[allow(
    clippy::too_many_arguments,
    reason = "forwards the start's own parameters one-to-one"
)]
pub async fn copy_between_volumes(
    engine: &Operations,
    events: Arc<dyn OperationEventSink>,
    source_volume_id: String,
    source_volume: Arc<dyn Volume>,
    source_paths: Vec<PathBuf>,
    dest_volume_id: String,
    dest_volume: Arc<dyn Volume>,
    dest_path: PathBuf,
    config: VolumeCopyConfig,
    initiator: Initiator,
    expected_sources: Option<ExpectedSources>,
) -> Result<WriteOperationStartResult, WriteOperationError> {
    engine
        .copy_between_volumes(
            events,
            source_volume_id,
            source_volume,
            app_paths_of(&source_paths),
            dest_volume_id,
            dest_volume,
            app_path_of(&dest_path),
            config,
            initiator,
            expected_sources,
        )
        .await
}

/// Starts one queued, same-volume batch rename on `engine`, as a reviewed batch
/// does.
pub fn start_bulk_rename(
    engine: &Operations,
    events: Arc<dyn OperationEventSink>,
    volume_id: String,
    rows: Vec<BulkRenameRow>,
    initiator: Initiator,
) -> Result<WriteOperationStartResult, RenameStartError> {
    engine.start_bulk_rename(events, volume_id, rows, initiator)
}

/// Sets the live pause gate of `operation_id` on `engine`, without the queue's
/// record flip: `true` when the operation was live.
pub fn pause_write_operation(engine: &Operations, operation_id: &OperationId) -> bool {
    engine.pause_write_operation(operation_id)
}

/// Clears the live pause gate of `operation_id` on `engine`: `true` when the
/// operation was live.
pub fn resume_write_operation(engine: &Operations, operation_id: &OperationId) -> bool {
    engine.resume_write_operation(operation_id)
}

// The Criterion bench's door: engines of its own, driven through the internal
// entry points a started operation runs.
pub mod bench;
