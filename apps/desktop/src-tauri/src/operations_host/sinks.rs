//! The app's answers to the engine's two sink seams: each event becomes a Tauri
//! event (wrapped for the wire, `wire.rs`), and a terminal one carries its app
//! side effects first (`terminal_effects.rs`).
//!
//! Built at the IPC edge, one per operation or preview, and handed to the
//! engine; the engine never builds one itself.

use tauri::AppHandle;
use tauri_specta::Event;

use super::terminal_effects::{self, AppTerminalEffects};
use super::wire;
use cmdr_operations::{
    ArmedConflict, ConflictInfo, DryRunResult, OperationEventSink, ScanPreviewCancelledEvent, ScanPreviewCompleteEvent,
    ScanPreviewErrorEvent, ScanPreviewEventSink, ScanPreviewProgressEvent, ScanProgressEvent, WriteCancelledEvent,
    WriteCompleteEvent, WriteConflictResolvedEvent, WriteErrorEvent, WriteLockedSourcesEvent, WriteProgressEvent,
    WriteSettledEvent, WriteSourceItemDoneEvent, WriteSourcesGoneEvent,
};

/// Tauri-backed event sink: calls `app.emit()` for each event.
pub struct TauriEventSink {
    app: AppHandle,
}

impl TauriEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// The app side effects each terminal emit carries (`terminal_effects.rs`).
    fn effects(&self) -> AppTerminalEffects<'static> {
        AppTerminalEffects {
            operations: super::operations().ok(),
        }
    }
}

impl OperationEventSink for TauriEventSink {
    fn emit_progress(&self, event: WriteProgressEvent) {
        crate::shell_progress::note_progress(&self.app, &event);
        let _ = wire::WriteProgress(event).emit(&self.app);
    }
    fn emit_complete(&self, event: WriteCompleteEvent) {
        crate::shell_progress::note_completed(&self.app, &event.operation_id);
        terminal_effects::complete(&self.effects(), event, |event| {
            let _ = wire::WriteComplete(event).emit(&self.app);
        });
    }
    fn emit_cancelled(&self, event: WriteCancelledEvent) {
        terminal_effects::cancelled(&self.effects(), event, |event| {
            let _ = wire::WriteCancelled(event).emit(&self.app);
        });
    }
    fn emit_error(&self, event: WriteErrorEvent) {
        terminal_effects::error(&self.effects(), event, |event| {
            let _ = wire::WriteError(event).emit(&self.app);
        });
    }
    fn emit_conflict(&self, event: ArmedConflict) {
        let _ = wire::WriteConflict(event.into_event()).emit(&self.app);
    }
    fn emit_conflict_resolved(&self, event: WriteConflictResolvedEvent) {
        let _ = wire::WriteConflictResolved(event).emit(&self.app);
    }
    fn emit_source_item_done(&self, event: WriteSourceItemDoneEvent) {
        let _ = wire::WriteSourceItemDone(event).emit(&self.app);
    }
    fn emit_sources_gone(&self, event: WriteSourcesGoneEvent) {
        let _ = wire::WriteSourcesGone(event).emit(&self.app);
    }
    fn emit_locked_sources(&self, event: WriteLockedSourcesEvent) {
        let _ = wire::WriteLockedSources(event).emit(&self.app);
    }
    fn emit_scan_progress(&self, event: ScanProgressEvent) {
        let _ = wire::ScanProgress(event).emit(&self.app);
    }
    fn emit_scan_conflict(&self, conflict: ConflictInfo) {
        let _ = wire::ScanConflict(conflict).emit(&self.app);
    }
    fn emit_dry_run_complete(&self, result: DryRunResult) {
        let _ = wire::DryRunComplete(result).emit(&self.app);
    }
    fn emit_settled(&self, event: WriteSettledEvent) {
        terminal_effects::settled(&self.effects(), event, |event| {
            let _ = wire::WriteSettled(event).emit(&self.app);
        });
    }
}

/// Tauri-backed scan-preview sink: calls `app.emit()` for each event.
pub struct TauriScanPreviewSink {
    app: AppHandle,
}

impl TauriScanPreviewSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ScanPreviewEventSink for TauriScanPreviewSink {
    fn emit_progress(&self, event: ScanPreviewProgressEvent) {
        let _ = wire::ScanPreviewProgress(event).emit(&self.app);
    }
    fn emit_complete(&self, event: ScanPreviewCompleteEvent) {
        let _ = wire::ScanPreviewComplete(event).emit(&self.app);
    }
    fn emit_error(&self, event: ScanPreviewErrorEvent) {
        let _ = wire::ScanPreviewError(event).emit(&self.app);
    }
    fn emit_cancelled(&self, event: ScanPreviewCancelledEvent) {
        let _ = wire::ScanPreviewCancelled(event).emit(&self.app);
    }
}
