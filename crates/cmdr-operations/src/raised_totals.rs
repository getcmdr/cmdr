//! A progress event never shows more done than its total (ERR-BGEKC).
//!
//! The totals an operation's events carry are the scan's count, and the work
//! can outgrow it: a copy that follows a link the scan counted as one entry, a
//! file that grew after it was counted, a folder that gained files mid-copy. A
//! bar reading `23 GB / 6.8 GB (337%)` with "0s left" says the dialog lost track,
//! so [`RaisedTotals::apply`] (called from `WriteOperationState::enrich_progress`,
//! which every progress event passes) raises a total to what's done and marks the
//! event `outgrew_totals`, for the dialog to say it's still counting and for the
//! ETA to step aside. `DETAILS.md` § "A total never sits below what's done".

use std::sync::Mutex;

use cmdr_fs::ignore_poison::IgnorePoison;

use super::types::{WriteOperationPhase, WriteProgressEvent};

/// The highest total each axis has been raised to in the current phase.
// DEFAULT-OK: nothing raised yet is what an operation that hasn't emitted has raised.
#[derive(Default)]
pub(crate) struct RaisedTotals {
    current: Mutex<Option<Raised>>,
}

struct Raised {
    phase: WriteOperationPhase,
    files: usize,
    bytes: u64,
}

impl RaisedTotals {
    /// Raises `event`'s totals to at least what it has done, and to at least the
    /// highest they were raised to earlier in the same phase, so a rollback's
    /// draining bar drains from its first frame.
    ///
    /// Only an axis the emitter gave a total: zero means "not counted yet"
    /// (a scan, an unsized stream), and a bar against a made-up total would sit
    /// at 100%. Phases with no honest denominator pass untouched.
    pub(crate) fn apply(&self, event: &mut WriteProgressEvent) {
        if !has_counted_totals(event.phase) {
            return;
        }
        let mut current = self.current.lock_ignore_poison();
        let raised = match current.as_mut() {
            Some(raised) if raised.phase == event.phase => raised,
            _ => current.insert(Raised {
                phase: event.phase,
                files: 0,
                bytes: 0,
            }),
        };
        let counted = (event.files_total, event.bytes_total);
        if event.files_total > 0 {
            raised.files = raised.files.max(event.files_total).max(event.files_done);
            event.files_total = raised.files;
        }
        if event.bytes_total > 0 {
            raised.bytes = raised.bytes.max(event.bytes_total).max(event.bytes_done);
            event.bytes_total = raised.bytes;
        }
        // Removing what was copied counts down from what was done at Cancel: a
        // raised total there is the bar's starting point, not more work found.
        event.outgrew_totals =
            event.phase != WriteOperationPhase::RollingBack && (event.files_total, event.bytes_total) != counted;
    }
}

/// The phases whose events carry a scan's count as their totals.
fn has_counted_totals(phase: WriteOperationPhase) -> bool {
    match phase {
        WriteOperationPhase::Copying
        | WriteOperationPhase::Compressing
        | WriteOperationPhase::Transferring
        | WriteOperationPhase::Deleting
        | WriteOperationPhase::Trashing
        | WriteOperationPhase::RollingBack => true,
        WriteOperationPhase::Scanning
        | WriteOperationPhase::FinishingCompression
        | WriteOperationPhase::FinishingTransfer
        | WriteOperationPhase::Flushing => false,
    }
}

#[cfg(test)]
#[path = "raised_totals_tests.rs"]
mod tests;
