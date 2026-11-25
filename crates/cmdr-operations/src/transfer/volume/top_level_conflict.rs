//! The serial volume drivers' top-level conflict resolver: what the async
//! driver calls when a selected source's name is taken at the destination.
//!
//! The cross-volume copy (`copy_serial.rs`) and move (`move_cross.rs`) ask the
//! same question the same way, so they build it here from ONE capture set
//! instead of each walling its own in front of the closure, where the two
//! drifted on which captures the resolver got.

use cmdr_fs::app_path::AppPathBuf;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::super::super::conflict::ApplyToAll;
use super::super::super::event_sinks::OperationEventSink;
use super::super::super::state::WriteOperationState;
use super::super::super::types::{VolumeCopyConfig, WriteOperationError};
use super::super::transfer_driver::{ConflictDecision, ConflictDecisionInput, ResolveFut};
use super::super::transfer_probe::{DriverPhase, OperationProbe};
use super::conflict::resolve_volume_conflict;
use super::displaced_destination::DisplacedLedger;
use super::listed_name::std_of;
use super::preflight::SourceHint;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::Volume;

/// Everything the resolver reads, captured once per operation. Cloned per
/// clash: every field is an `Arc` or cheap to copy.
#[derive(Clone)]
pub(super) struct TopLevelConflicts {
    pub source_volume: Arc<dyn Volume>,
    pub dest_volume: Arc<dyn Volume>,
    pub state: Arc<WriteOperationState>,
    pub events: Arc<dyn OperationEventSink>,
    /// The OPERATION's "…all" latch, shared with the deep merge.
    pub apply_to_all: Arc<Mutex<ApplyToAll>>,
    /// The preflight's per-source hints, so a clash never re-probes the source.
    pub source_hints: Arc<HashMap<AppPathBuf, SourceHint>>,
    pub config: VolumeCopyConfig,
    pub operation_id: String,
    /// Where the driver says it's waiting on a person. `None` with no probe.
    pub op_probe: Option<Arc<OperationProbe>>,
    /// Where a cross-type Overwrite's renamed-aside entry is held until the
    /// operation settles it (`displaced_destination.rs`).
    pub displaced: Arc<DisplacedLedger>,
    /// The driver's name in the log lines.
    pub log_label: &'static str,
}

impl TopLevelConflicts {
    /// The closure `drive_transfer_serial_async` takes as its
    /// `conflict_resolver`.
    pub(super) fn resolver(self) -> impl for<'a> FnMut(ConflictDecisionInput<'a>) -> ResolveFut<'a> {
        move |input: ConflictDecisionInput<'_>| -> ResolveFut<'_> {
            let conflicts = self.clone();
            let source_path = input.source_path.to_owned();
            let initial_dest = input.initial_dest_path.to_owned();
            let dest_size_hint = input.dest_size_hint;
            Box::pin(async move { conflicts.resolve(source_path, initial_dest, dest_size_hint).await })
        }
    }

    async fn resolve(
        self,
        source_path: AppPathBuf,
        initial_dest: AppPathBuf,
        dest_size_hint: Option<u64>,
    ) -> Result<ConflictDecision, WriteOperationError> {
        // The cached scan hint, so a clash never re-probes the source (on MTP,
        // a parent listing per conflict). `Some` only when the preflight
        // produced one, so the resolver keeps its trait-call fallback rather
        // than trusting a defaulted `false`.
        let source_hint = self.source_hints.get(&source_path).copied();
        let source_size_hint = source_hint.and_then(|h| (!h.is_directory).then_some(h.size));
        let source_is_directory_hint = source_hint.map(|h| h.is_directory);
        log::debug!(
            "{}: conflict detected at {} (source_is_directory_hint={:?})",
            // allowed-log-private: `log_label` is the driver's own fixed name, not a user value
            self.log_label,
            LogPrivate::path(initial_dest.as_str()),
            source_is_directory_hint,
        );
        // The probe's step text still takes a `std::path` value.
        let initial_std = std_of(&initial_dest);
        // Parked on a PERSON, unbounded by design and with nothing else
        // running. A dump that names it has explained the whole stall;
        // `paused=` covers the pause gate and nothing else covered this.
        if let Some(probe) = self.op_probe.as_ref() {
            probe.set_driver_phase_at(DriverPhase::ResolvingConflict, None, &initial_std);
        }
        let resolved = resolve_volume_conflict(
            &self.source_volume,
            &source_path,
            &self.dest_volume,
            &initial_dest,
            &self.config,
            &*self.events,
            &self.operation_id,
            &self.state,
            &self.apply_to_all,
            source_size_hint,
            dest_size_hint,
            source_is_directory_hint,
        )
        .await
        .map_err(|e| e.into_write_error())?;
        Ok(match resolved {
            None => {
                log::debug!(
                    "{}: skipping {} due to conflict resolution",
                    // allowed-log-private: `log_label` is the driver's own fixed name, not a user value
                    self.log_label,
                    LogPrivate::path(source_path.as_str())
                );
                // Credit the source's byte size so the size bar matches the
                // file counter when every source is skipped. Dirs report 0 in
                // `source_hints` by convention (the recursive total isn't
                // tracked there); per-file skips credit the real size.
                let bytes_accounted = source_hint.map(|h| h.size).unwrap_or(0);
                ConflictDecision::Skip { bytes_accounted }
            }
            Some(rc) => {
                // The operation settles a cross-type aside when it ends; the
                // item's own ending can't tell.
                if let Some(aside) = rc.displaced {
                    self.displaced.hold(aside);
                }
                ConflictDecision::Proceed {
                    dest_path: rc.write_path,
                    replaces: rc.replaces,
                    dest_name_claimed: rc.reserved_placeholder,
                }
            }
        })
    }
}
