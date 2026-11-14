//! How one file of a local copy settles: counted done or skipped (with its
//! progress milestone), asked about when the destination refuses its name, and,
//! once it landed, journaled and recorded in the in-flight ledger.
//!
//! The per-file bookkeeping `copy_single_item` (`single_item.rs`) calls at every
//! exit, kept apart from the copy flow so the two landing branches (a symlink
//! carried as-is, a regular file) settle through one door and can't drift.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::event_sinks::OperationEventSink;
use crate::ledger::{CopyTransaction, WrittenFile};
use crate::refused_name::{RefusedItem, RefusedNameAnswer, ask_blocking, refusal_reason};
use crate::scan_source_tracker::FileVerdict;
use crate::state::WriteOperationState;
use crate::transfer_tree::TreeLeaf;
use crate::types::{WriteOperationError, WriteOperationPhase, WriteOperationType};

/// Operation-wide context for the per-file milestone emit. Bundled into one
/// struct so the six emit sites in `copy_single_item` don't each restate
/// ten arguments to [`record_file_done`].
pub(super) struct PerFileCtx<'a> {
    pub(super) events: &'a dyn OperationEventSink,
    pub(super) state: &'a Arc<WriteOperationState>,
    pub(super) operation_id: &'a str,
    pub(super) operation_type: WriteOperationType,
    pub(super) files_total: usize,
    pub(super) bytes_total: u64,
    /// The file's transfer-tree row.
    pub(super) row: &'a TreeLeaf,
}

/// Marks one file as completed: settles its tree row by `verdict`, bumps the
/// cumulative counters, and emits a `Copying`-phase `WriteProgressEvent`
/// carrying the bumped values. The row settles first, so a view pulled on
/// that event already shows it.
///
/// Called from every `Ok`-return site in `copy_single_item` (regular file
/// copy, symlink copy, per-file Skip, type-mismatch parent Skip, same-file
/// no-op). Owning the milestone here — rather than in the driver's
/// `Transferred` arm — means both `copy_files_with_progress_inner` (which
/// goes through `drive_transfer_serial_sync`) and `move_with_staging` (which
/// calls `copy_single_item` directly inside its own copy loop) see the same
/// per-file milestone shape. Without that, single-file ops never see the FE's
/// files-done axis cross `0/1` before the dialog closes on the complete
/// event, because the chunked-copy callback (or an instant clonefile) leaves
/// the axis snapshotted at the pre-iteration value.
///
/// Fires unconditionally (no throttle): per-file milestones are bounded by
/// file count, and throttle suppression of this specific event is the bug
/// being fixed. The chunked intra-file emit inside `copy_single_item` keeps
/// its own throttle for the byte-axis stream.
pub(super) fn record_file_done(
    ctx: &PerFileCtx<'_>,
    source: &Path,
    write_weight: u64,
    files_done: &mut usize,
    bytes_done: &mut u64,
    verdict: FileVerdict,
) {
    match verdict {
        FileVerdict::CarriedOut => ctx.row.complete(),
        FileVerdict::Skipped => ctx.row.skip(),
    }
    *files_done += 1;
    *bytes_done += write_weight;
    super::super::transfer_driver::emit_progress_and_status(
        ctx.events,
        ctx.state,
        ctx.operation_id,
        ctx.operation_type,
        WriteOperationPhase::Copying,
        source.file_name().map(|n| n.to_string_lossy().to_string()),
        *files_done,
        ctx.files_total,
        *bytes_done,
        ctx.bytes_total,
    );
}

/// A file write that failed with `error`: when the destination refused the
/// file's NAME, asks the person (`crate::refused_name`), and a Skip counts the
/// file as skipped. Anything else, a Stop included, is the failure it was.
///
/// Nothing of the file is at the destination by then and nothing is in the
/// ledger, so a cross-FS move's sweep (which deletes the ledger) keeps its
/// source.
pub(super) fn skip_if_refused(
    error: WriteOperationError,
    source: &Path,
    dest: &Path,
    write_weight: u64,
    ctx: &PerFileCtx<'_>,
    files_done: &mut usize,
    bytes_done: &mut u64,
) -> Result<FileVerdict, WriteOperationError> {
    let Some(reason) = refusal_reason(&error) else {
        return Err(error);
    };
    let item = RefusedItem {
        operation_id: ctx.operation_id,
        source_path: source,
        dest_path: dest,
        source_size: Some(write_weight),
        source_is_directory: false,
        reason,
    };
    match ask_blocking(ctx.state, ctx.events, &item) {
        RefusedNameAnswer::Skip => {
            record_file_done(ctx, source, write_weight, files_done, bytes_done, FileVerdict::Skipped);
            Ok(FileVerdict::Skipped)
        }
        RefusedNameAnswer::Stop => Err(error),
        RefusedNameAnswer::Cancelled => Err(WriteOperationError::Cancelled {
            message: "Operation cancelled by user".into(),
        }),
    }
}

/// Where a landed file should be RECORDED when the write goes somewhere else
/// first.
///
/// A cross-FS move copies into `.cmdr-staging-<op_id>/` and renames the tree into
/// place only afterwards, so a journal row naming the write path names a
/// directory that is gone moments later: name search points into nothing, and a
/// reversal reads the item as already gone and counts a phantom success. The
/// caller hands over the two roots and the record points rebase.
///
/// Rebasing rather than taking a ready-made final path is deliberate: it keeps a
/// landing that DEVIATED inside `copy_single_item` (a conflict, a `dir_remap`
/// redirect) pointing at the matching final path.
///
/// It records where the file will live if nothing else moves it. Phase 3 of a
/// cross-FS move can still resolve a conflict at the real destination and land
/// the file elsewhere — that operation marks itself not-rollbackable, because no
/// row written here can be corrected after the fact.
#[derive(Clone, Copy)]
pub(in crate::transfer) struct JournalDestUnder<'a> {
    /// The root the bytes are written under (the staging directory).
    pub write_root: &'a Path,
    /// The root the file will live under once the caller renames it into place.
    pub final_root: &'a Path,
}

impl JournalDestUnder<'_> {
    /// The destination to record for a write that landed at `written`.
    fn rebase(&self, written: &Path) -> PathBuf {
        match written.strip_prefix(self.write_root) {
            Ok(rel) => self.final_root.join(rel),
            // Can't happen (every write lands under `write_root`), and recording
            // the written path beats dropping the row.
            Err(_) => written.to_path_buf(),
        }
    }
}

/// The destination `record_local_leaf` should carry for a file that landed at
/// `written`: the rebased path when the caller staged the write, else the path
/// itself.
fn journal_dest(remap: Option<JournalDestUnder<'_>>, written: &Path) -> PathBuf {
    remap.map_or_else(|| written.to_path_buf(), |r| r.rebase(written))
}

/// Settles a leaf (a file, or a symlink carried as-is) that landed at
/// `actual_dest`: journals it, records it in the in-flight ledger, and counts it
/// done.
///
/// Call it only after the item's last write, so the landed node is what stays.
/// It snapshots that node so a reversal (the in-flight one, and an undo from
/// history) can tell it apart from anything that replaced it since: one `lstat`
/// on a path just written, shared by the journal row and the ledger entry. A
/// failed `lstat` leaves both without an identity; it never fails the op.
#[allow(
    clippy::too_many_arguments,
    reason = "The per-file copy's accounting state, passed through from `copy_single_item`"
)]
pub(super) fn settle_landed_leaf(
    ctx: &PerFileCtx<'_>,
    transaction: &mut CopyTransaction,
    journal_dest_under: Option<JournalDestUnder<'_>>,
    source: &Path,
    actual_dest: PathBuf,
    source_mtime: Option<i64>,
    overwrote: bool,
    write_weight: u64,
    files_done: &mut usize,
    bytes_done: &mut u64,
) {
    let landed_meta = fs::symlink_metadata(&actual_dest).ok();
    crate::journal::record_local_leaf(
        ctx.state.scope(),
        cmdr_operation_log::types::EntryType::File,
        source,
        Some(&journal_dest(journal_dest_under, &actual_dest)),
        Some(write_weight as i64),
        source_mtime,
        overwrote,
        cmdr_operation_log::types::ItemOutcome::Done,
        crate::journal_landed::landed_node(ctx.state.scope(), &actual_dest, landed_meta.as_ref()),
    );
    transaction.record_file(WrittenFile::local_stat(actual_dest, landed_meta.as_ref()));
    record_file_done(
        ctx,
        source,
        write_weight,
        files_done,
        bytes_done,
        FileVerdict::CarriedOut,
    );
}
