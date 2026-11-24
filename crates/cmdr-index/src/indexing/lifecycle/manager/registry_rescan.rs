//! Rescanning a volume through the registry: the door every "this volume needs a
//! fresh scan" trigger below the manager comes back in through, re-resolving the
//! manager by volume id rather than holding one.

use super::super::state::{self, Handover};
use crate::indexing::watch::event_loop::LiveLoopEnd;

/// Take a volume's manager OUT of the registry (transient `ShuttingDown`), stop
/// its current watcher + live loop, run a fresh `start_scan` OFF the registry
/// lock, then reinsert it as `Running`. Re-resolves the manager by volume id, so
/// it can be spawned fire-and-forget with no captured manager.
///
/// Shared by the triggers that all mean "roll forward from the visible scanner,
/// not the invisible reconcile": the cold-start replay full-scan fallback
/// (`start_replay`), a live loop's ingestion backlog ([`after_live_loop`]), and
/// the shallow-`MustScanSubDirs` scanner routing
/// (`reconcile/reconciler/rescan/mod.rs`). Single-flight: `start_scan` no-ops if a
/// scan is already running, so overlapping triggers coalesce.
///
/// Runs the blocking `start_scan` prelude off the lock (holding it across
/// `flush_blocking` + the space-info query would freeze every concurrent registry
/// user; the freshness firing inside `start_scan` would also re-lock the registry,
/// now fired through the manager's own `Arc`). Mirrors `state::force_scan`'s
/// extract-drop-run-reinsert flow.
pub(in crate::indexing) async fn perform_registry_rescan(volume_id: &str, trigger: &str) {
    let why = format!("a registry rescan ({trigger})");
    let handover = state::off_the_registry(volume_id, &why, |mgr| {
        // Retire the current watcher and live loop (the fresh scan starts its own).
        // Off the lock with the rest: stopping a watcher joins its run-loop thread.
        if let Some(mut watcher) = mgr.retire_the_watcher() {
            watcher.stop();
        }
        // The blocking-prelude scan start. The same door "Rescan now" goes
        // through, so a volume the machine is still building has its phases
        // restarted rather than its half-built index truncated.
        if let Err(ref e) = mgr.cover_or_scan(trigger) {
            log::warn!("Scanner rescan for '{volume_id}' failed to start: {e}");
        }
    });
    match handover {
        Err(e) => log::debug!("Scanner rescan for '{volume_id}' found nothing running: {e}"),
        Ok(Handover::Restored(())) => {}
        Ok(Handover::TornDownWhileAway(())) => {
            log::info!("scanner rescan: '{volume_id}' was torn down during scan start; the teardown ran instead");
        }
    }
}

/// Run whatever a volume's live loop left owing when it ended.
///
/// Every task that runs a live loop ends through here, because the loop lives in
/// `watch/`, below the registry, and can only say that the volume needs a fresh
/// scan. The rescan is spawned, never awaited in place: it retires this volume's
/// live loop by aborting its task, which is the task calling this.
pub(in crate::indexing::lifecycle) fn after_live_loop(volume_id: String, end: LiveLoopEnd) {
    match end {
        LiveLoopEnd::Stopped => {}
        LiveLoopEnd::IngestionBacklog => {
            crate::indexing::host::runtime::spawn(async move {
                perform_registry_rescan(&volume_id, "ingestion backlog").await;
            });
        }
        LiveLoopEnd::WatchLost { lived } => {
            crate::indexing::host::runtime::spawn(super::watch_rearm::rearm_after_watch_lost(volume_id, lived));
        }
    }
}
