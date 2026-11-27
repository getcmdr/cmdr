//! Trash implementation for write operations.
//!
//! Provides `move_to_trash_sync()` (reusable core) and `trash_files_with_progress()`
//! (batch operation with progress, cancellation, and partial failure support).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_operation_log::types::OperationId;

use super::super::event_sinks::OperationEventSink;
use super::super::mutation_error::MutationError;
use super::super::state::WriteOperationState;
use super::super::types::{
    CancelRollback, SourceItemOutcome, TrashRefusalKind, TrashRefusedItems, WriteCancelledEvent, WriteCompleteEvent,
    WriteErrorEvent, WriteOperationError, WriteOperationPhase, WriteOperationType, WriteProgressEvent,
    WriteSourceItemDoneEvent,
};
use cmdr_fs::error_detail::ErrorDetail;
use cmdr_fs::log_error::{ErrorLogText, ErrorText, LogError, write_plain};
use cmdr_fs::log_private::LogPrivate;

// ============================================================================
// Core trash function (reusable by commands/rename.rs and batch trash)
// ============================================================================

mod platform;

pub(crate) use platform::forget_restored;
pub use platform::{move_to_trash_sync, trash_dir_for_path};

/// What the sticky-folder rule looks at, as plain numbers so the rule is testable
/// without a second account on the machine.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy)]
struct StickyFacts {
    folder_mode: u32,
    folder_owner: u32,
    item_owner: u32,
    us: u32,
}

/// The kernel's sticky-folder rule (`S_ISVTX`, sticky(7)): it refuses a rename
/// unless we're root or own the item or the folder.
#[cfg(any(target_os = "macos", test))]
fn sticky_rule_refuses(facts: StickyFacts) -> bool {
    const S_ISVTX: u32 = 0o1000;
    facts.folder_mode & S_ISVTX != 0 && facts.us != 0 && facts.item_owner != facts.us && facts.folder_owner != facts.us
}

impl super::super::Operations {
    /// Trash ONE item and journal it as a one-item trash operation — the
    /// single-trash path (`commands/rename.rs::move_to_trash`), which bypasses the
    /// batch trash and its manager op. Mirrors the batch path's capture: enumerate
    /// the subtree's `search_only` leaves from the drive index BEFORE the OS move
    /// (the reconciler prunes on the FSEvent), record the top-level
    /// `rollback_unit` row with the in-trash dest, persist the leaves only on
    /// success, and finalize. Returns the in-trash location like
    /// [`move_to_trash_sync`]. Runs on a `spawn_blocking` thread (sync index reads
    /// + journal sends are fine there).
    pub fn trash_single_journaled(
        &self,
        source: &Path,
        initiator: cmdr_operation_log::types::Initiator,
    ) -> Result<Option<PathBuf>, MutationError> {
        use cmdr_operation_log::types::{EntryType, ExecutionStatus, OpKind};
        let scope = self.op_scope();
        // `x/link/` names the link, ❌ never its target (`wire_paths.rs`): the `lstat`
        // below would follow it, and journal the target's subtree as the trashed item's.
        let source = &*crate::wire_paths::without_trailing_separators(source);

        let source_meta = fs::symlink_metadata(source).map_err(|_| MutationError::NotFound {
            path: source.display().to_string(),
        })?;

        // Journaled under the drive holding it, as the batch trash is (#1433): `root` on a
        // Mac, `D:`'s own volume on Windows.
        if let Some((volume_id, root)) = crate::local_start::one_local_side(self.host().volumes(), source) {
            scope.journal_local_sides((&volume_id, &root), (&volume_id, &root));
        }

        // Enumerate BEFORE the move (buffered, persisted only on success).
        let buffered = if source_meta.is_dir() {
            Some(super::super::journal_search::enumerate_subtree_for_search(
                scope.host().index(),
                scope.local_ids_for(source, None).0,
                source,
                super::super::journal_search::SEARCH_LEAF_CAP,
            ))
        } else {
            None
        };

        let op_id = OperationId::from(cmdr_operation_log::new_operation_id());
        // A single trash has no destination volume (the in-trash location rides on the
        // item row, not the header); one top-level item.
        super::super::journal::open_local_op(&scope, op_id.as_str(), OpKind::Trash, initiator, 1, None);

        match move_to_trash_sync(source) {
            Ok(in_trash) => {
                let entry_type = if source_meta.is_dir() {
                    EntryType::Dir
                } else {
                    EntryType::File
                };
                super::super::journal::record_trashed(
                    &scope,
                    entry_type,
                    source,
                    in_trash.as_deref(),
                    Some(source_meta.len() as i64),
                    super::super::journal::mtime_secs(&source_meta),
                );
                if let Some(buffered) = &buffered {
                    let (source_volume, trash_volume) = scope.local_ids_for(source, in_trash.as_deref());
                    super::super::journal_search::persist_and_note(
                        &scope,
                        source_volume,
                        source,
                        trash_volume,
                        in_trash.as_deref(),
                        buffered,
                    );
                }
                // Its one item is done: no status row counts it (#1433).
                super::super::journal::finalize_op_reporting(&scope, OpKind::Trash, ExecutionStatus::Done, 1);
                Ok(in_trash)
            }
            Err(e) => {
                super::super::journal::finalize_op(&scope, OpKind::Trash, ExecutionStatus::Failed);
                Err(e)
            }
        }
    }
}

// ============================================================================
// Batch trash with progress
// ============================================================================

/// A refused batch's summary for `TrashRefused`'s technical details: `path: why`, joined by
/// `; `. Typed, so a log line marks each path.
fn refused_summary(errors: &[TrashItemError]) -> ErrorDetail {
    ErrorDetail::of(RefusedSummary(
        errors.iter().map(|e| (e.path.clone(), e.message.clone())).collect(),
    ))
}

/// [`refused_summary`]'s cause: each refused item's path and the OS's words about it.
struct RefusedSummary(Vec<(PathBuf, ErrorDetail)>);

impl std::fmt::Display for RefusedSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_plain(self, f)
    }
}

impl ErrorLogText for RefusedSummary {
    fn write_log_text(&self, out: &mut ErrorText<'_>) -> std::fmt::Result {
        use std::fmt::Write as _;
        for (index, (path, message)) in self.0.iter().enumerate() {
            if index > 0 {
                out.write_str("; ")?;
            }
            out.value(LogPrivate::path(path))?;
            out.write_str(": ")?;
            out.error(message)?;
        }
        Ok(())
    }
}

/// Per-item error from a trash operation.
#[derive(Debug, Clone)]
pub(crate) struct TrashItemError {
    pub path: PathBuf,
    /// The OS's words, or the typed refusal, which marks its own paths in a log line.
    pub message: ErrorDetail,
    /// The OS's error codes for this refusal (`MutationError::TrashRefused::codes`), for the
    /// batch's [`refused_codes`].
    pub codes: Option<String>,
    /// Why the OS refused this one, kept as a value so the batch's terminal error can
    /// carry it. Without it the whole batch collapses to one sentence and the dialog
    /// has nothing to offer but "try again".
    pub reason: TrashRefusalKind,
    /// This item's contents were only on the provider's servers, which is what
    /// the refusal was really about. Stat'ed at the refusal, while the item is
    /// still there to ask. See `cloud_trash.rs`.
    pub online_only: bool,
}

/// Moves files to trash with progress reporting, cancellation, and partial failure.
///
/// Iterates top-level items, calling `move_to_trash_sync()` for each.
/// Unlike permanent delete, trash doesn't need a recursive scan phase because
/// `trashItemAtURL` is atomic per top-level item (the OS moves the entire tree).
///
/// # Arguments
/// * `events` - Event sink for `write-progress`, `write-complete`, `write-cancelled`,
///   `write-error`, and `write-source-item-done` emits. Production wraps a Tauri AppHandle
///   in `TauriEventSink`; tests use `CollectorEventSink`.
/// * `operation_id` - Unique operation ID for event correlation
/// * `state` - Shared state with cancellation flag and progress interval
/// * `sources` - Top-level items to trash
/// * `item_sizes` - Optional per-item sizes for byte-level progress (from scan preview or drive
///   index). When `None`, bytes progress is not reported.
pub(crate) fn trash_files_with_progress(
    events: &dyn OperationEventSink,
    operation_id: &str,
    state: &Arc<WriteOperationState>,
    sources: &[PathBuf],
    item_sizes: Option<&[u64]>,
) -> Result<(), WriteOperationError> {
    let items_total = sources.len();
    let bytes_total: u64 = item_sizes.map(|s| s.iter().sum()).unwrap_or(0);

    let mut items_done = 0usize;
    let mut bytes_done = 0u64;
    let mut errors: Vec<TrashItemError> = Vec::new();
    let mut last_progress_time = crate::rt::now();

    for (i, source) in sources.iter().enumerate() {
        // The cooperative boundary, between items. Trash hands a whole tree to
        // the OS in one call, so this is the only place it can stop, and it's
        // the same boundary the two delete walkers park at.
        if state.stop_or_park_sync() {
            events.emit_cancelled(WriteCancelledEvent {
                operation_id: operation_id.to_string(),
                operation_type: WriteOperationType::Trash,
                files_processed: items_done,
                rollback: CancelRollback::none(), // Trash is recoverable, no rollback needed
            });
            return Err(WriteOperationError::Cancelled {
                message: "Operation cancelled by user".into(),
            });
        }

        // Check existence using symlink_metadata (handles dangling symlinks).
        // Keep the metadata: it's the free snapshot (kind + mtime) the journal
        // records for the top-level item, stat'd BEFORE the OS moves it to trash.
        let source_meta = match fs::symlink_metadata(source) {
            Ok(m) => m,
            Err(error) => {
                errors.push(TrashItemError {
                    path: source.clone(),
                    message: "no longer exists".into(),
                    // A vanished source is not a permission problem; offering a grant
                    // as the fix would send the user somewhere useless.
                    reason: TrashRefusalKind::Other,
                    codes: None,
                    // We couldn't stat it at all, so there are no flags to read.
                    online_only: false,
                });
                emit_item_failed(
                    events,
                    operation_id,
                    source,
                    // Only a NotFound proves the item is gone. "We couldn't look"
                    // (permissions, a dead mount) is not evidence, and this flag
                    // decides whether every search snapshot drops the row.
                    error.kind() == std::io::ErrorKind::NotFound,
                );
                continue;
            }
        };

        // Defensive: register with the downloads watcher's ignore set so a
        // future "deleted from Downloads" event source wouldn't surprise us.
        // No-ops outside ~/Downloads.
        state.scope().host().announce(source);

        // Enumerate the subtree's `search_only` leaves from the drive index BEFORE
        // the OS move — the index reconciler prunes the subtree the instant it sees
        // the trash FSEvent, so a later read would miss them (search-leaf enumeration). Only a directory
        // has descendants; a file is fully covered by its top-level row. Buffered
        // now, persisted only after this item succeeds (below).
        let buffered_leaves = if source_meta.is_dir() {
            Some(super::super::journal_search::enumerate_subtree_for_search(
                state.scope().host().index(),
                // The index of the drive holding it: `root` on a Mac, `D:`'s own on Windows.
                state.scope().local_ids_for(source, None).0,
                source,
                super::super::journal_search::SEARCH_LEAF_CAP,
            ))
        } else {
            None
        };

        // Attempt to trash the item
        match move_to_trash_sync(source) {
            Ok(in_trash) => {
                items_done += 1;
                let item_size = item_sizes.and_then(|s| s.get(i).copied());
                if let Some(size) = item_size {
                    bytes_done += size;
                }

                // Journal the trashed top-level item as the rollback unit (one
                // restore-from-trash reverses the whole subtree). The in-trash
                // location (`resultingItemURL`) is the row's dest so the rollback restore
                // knows where to move it back FROM. The subtree's `search_only`
                // leaves are enumerated from the drive index (search-leaf enumeration).
                let entry_type = if source_meta.is_dir() {
                    cmdr_operation_log::types::EntryType::Dir
                } else {
                    cmdr_operation_log::types::EntryType::File
                };
                super::super::journal::record_trashed(
                    state.scope(),
                    entry_type,
                    source,
                    in_trash.as_deref(),
                    item_size.map(|s| s as i64).or(Some(source_meta.len() as i64)),
                    super::super::journal::mtime_secs(&source_meta),
                );

                // Persist the buffered `search_only` leaves NOW that this item
                // succeeded (never before — a failed item must record no leaves,
                // else search would return a trash that never happened). Their dest
                // is rebased onto the in-trash location; coverage downgrades are
                // noted worst-wins.
                if let Some(buffered) = &buffered_leaves {
                    let (source_volume, trash_volume) = state.scope().local_ids_for(source, in_trash.as_deref());
                    super::super::journal_search::persist_and_note(
                        state.scope(),
                        source_volume,
                        source,
                        trash_volume,
                        in_trash.as_deref(),
                        buffered,
                    );
                }

                events.emit_source_item_done(WriteSourceItemDoneEvent {
                    operation_id: operation_id.to_string(),
                    source_path: source.display().to_string(),
                    // `trashItemAtURL` moved the whole tree, so the original path
                    // is gone.
                    source_removed: true,
                    outcome: SourceItemOutcome::Done,
                });
            }
            Err(e) => {
                errors.push(TrashItemError {
                    path: source.clone(),
                    reason: match &e {
                        MutationError::TrashRefused { reason, .. } => *reason,
                        _ => TrashRefusalKind::Other,
                    },
                    codes: match &e {
                        MutationError::TrashRefused { codes, .. } => codes.clone(),
                        _ => None,
                    },
                    // The batch path reports through `WriteOperationError`, whose
                    // own typed variant carries the words; this is the
                    // technical detail beside it, kept typed so a log line marks its paths.
                    message: ErrorDetail::of(e.clone()),
                    // `trashItemAtURL` is atomic, so a refusal left the item
                    // exactly where it was and its flags are still readable.
                    online_only: super::cloud_trash::is_online_only(source),
                });
                // `trashItemAtURL` is atomic per item, so a failure left this one
                // exactly where it was.
                emit_item_failed(events, operation_id, source, false);
                continue;
            }
        }

        // Emit throttled progress
        if crate::rt::since(last_progress_time) >= state.progress_interval {
            let current_file = source.file_name().map(|n| n.to_string_lossy().to_string());
            state.emit_progress_via_sink(
                events,
                WriteProgressEvent::new(
                    operation_id.to_string(),
                    WriteOperationType::Trash,
                    WriteOperationPhase::Trashing,
                    current_file.clone(),
                    items_done,
                    items_total,
                    bytes_done,
                    bytes_total,
                ),
            );
            state.scope().status().update(
                operation_id,
                WriteOperationPhase::Trashing,
                current_file,
                items_done,
                items_total,
                bytes_done,
                bytes_total,
            );
            last_progress_time = crate::rt::now();
        }
    }

    // No fsync after trashing: like delete, a non-durable trash fails
    // annoyance-class (a trashed file could reappear after a crash, the user
    // re-trashes; never data loss), so targeted fsync isn't worth its cost, and
    // dropping the old whole-machine global sync (`sync(2)`) removes the stall
    // it caused on unrelated apps. See `CLAUDE.md` § "Durability".

    note_unwritable_trash(&errors);

    // If all items failed, emit error
    if items_done == 0 && !errors.is_empty() {
        let error_summary = refused_summary(&errors);
        // One reason for the batch: the strongest one any item reported. A batch where
        // even one item was refused for permission is a batch the FDA hint can help with,
        // and `Other` would silently withhold it.
        let reason = strongest_refusal(&errors);
        let failure = WriteOperationError::TrashRefused {
            item_count: errors.len(),
            reason,
            codes: refused_codes(&errors),
            message: error_summary,
            // One evicted item is enough to make the FDA advice wrong for this
            // batch, the same all-or-nothing stance the routing takes.
            online_only: errors.iter().any(|e| e.online_only),
        };
        events.emit_error(WriteErrorEvent::new(
            operation_id.to_string(),
            WriteOperationType::Trash,
            failure.clone(),
        ));
        return Err(failure);
    }

    // A batch that took some items and was refused others is neither a success
    // nor a failure. Reporting it as a plain success left the refused items
    // sitting in the pane with nothing said about them, so the completion carries
    // them: the count the FE names, and the same `strongest_refusal` the
    // all-refused error reports, so the wording can offer the same way through.
    // ❌ Not `files_skipped`, which means a deliberate skip.
    let refused = (!errors.is_empty()).then(|| TrashRefusedItems {
        item_count: errors.len() as u32,
        reason: strongest_refusal(&errors),
    });

    // Emit completion (may include partial errors)
    events.emit_complete(WriteCompleteEvent {
        operation_id: operation_id.to_string(),
        operation_type: WriteOperationType::Trash,
        files_processed: items_done,
        files_skipped: 0,
        bytes_processed: bytes_done,
        appeared_during_move: None,
        top_level_skipped: None,
        refused,
        metadata_left_behind: None,
        durability_unconfirmed: None,
        locked_kept: None,
        already_gone: None,
    });

    // Log partial failures
    if !errors.is_empty() {
        log::warn!(
            "Trash operation {} completed with {} errors out of {} items",
            operation_id,
            errors.len(),
            items_total
        );
        for error in &errors {
            log::warn!(
                "  Failed: {}: {}",
                LogPrivate::path(&error.path),
                LogError(&error.message)
            );
        }
    }

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

/// The reason a whole failed batch reports, from its items' reasons.
///
/// Takes the one that opens the most doors for the user rather than the most common:
/// a batch of nine items where one was refused for permission is still a batch where
/// granting a permission might help, and the majority answer would hide that. The
/// reasons read from the item and where it sits (Locked, a folder that takes no
/// writes, a sticky folder, a Trash that takes no writes, a program holding the item
/// open) come next: each names a concrete fix the user can make.
/// Each distinct OS code chain the refused items carried, once, in the order they met them, at
/// most [`MAX_REFUSED_CODE_CHAINS`]: a batch refused alike (one Trash that takes no writes) reads
/// as one chain, and a mixed one stays short enough to fit the report's `detail=` window.
fn refused_codes(errors: &[TrashItemError]) -> Option<String> {
    let mut distinct: Vec<&str> = Vec::new();
    for codes in errors.iter().filter_map(|e| e.codes.as_deref()) {
        if !distinct.contains(&codes) {
            distinct.push(codes);
        }
        if distinct.len() == MAX_REFUSED_CODE_CHAINS {
            break;
        }
    }
    (!distinct.is_empty()).then(|| distinct.join("; "))
}

/// How many distinct code chains a batch refusal names.
const MAX_REFUSED_CODE_CHAINS: usize = 3;

/// Once per batch an item of which refused as [`TrashRefusalKind::TrashNotWritable`]: what that
/// Trash folder says about itself, so a report tells the migration case (a Trash still owned by
/// an old account's uid) from a mode or flag change. Non-identifying by construction: numbers,
/// and whether it's the home Trash or a drive's `.Trashes/<uid>` (cmdr-reports#121).
fn note_unwritable_trash(errors: &[TrashItemError]) {
    let Some(item) = errors.iter().find(|e| e.reason == TrashRefusalKind::TrashNotWritable) else {
        return;
    };
    let Some(trash) = trash_dir_for_path(&item.path) else {
        return;
    };
    log::info!(target: "trash", "the Trash took no writes: {}", platform::trash_folder_facts(&trash));
}

fn strongest_refusal(errors: &[TrashItemError]) -> TrashRefusalKind {
    const RANKED: [TrashRefusalKind; 7] = [
        TrashRefusalKind::NotPermitted,
        TrashRefusalKind::Locked,
        TrashRefusalKind::FolderNotWritable,
        TrashRefusalKind::OwnedByAnotherUser,
        TrashRefusalKind::TrashNotWritable,
        TrashRefusalKind::InUse,
        TrashRefusalKind::NoTrashForVolume,
    ];
    RANKED
        .into_iter()
        .find(|kind| errors.iter().any(|e| e.reason == *kind))
        .unwrap_or(TrashRefusalKind::Other)
}

/// One top-level item this trash could not take.
///
/// Trash is per-item: one failure leaves the rest of the batch running, so the
/// operation's own terminal event says nothing about THIS item. Without a verdict
/// here, a caller tracking per-source outcomes waits forever for one that never
/// comes.
fn emit_item_failed(events: &dyn OperationEventSink, operation_id: &str, source: &Path, source_removed: bool) {
    events.emit_source_item_done(WriteSourceItemDoneEvent {
        operation_id: operation_id.to_string(),
        source_path: source.display().to_string(),
        source_removed,
        outcome: SourceItemOutcome::Failed,
    });
}

#[cfg(test)]
#[path = "trash_tests.rs"]
mod tests;
