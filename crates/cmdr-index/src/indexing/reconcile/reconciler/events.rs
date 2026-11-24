//! Turning ONE filesystem event into index writes.
//!
//! Shared by live mode and both replays. Normalizes the path, checks exclusions,
//! stats the target, resolves to integer entry ids, and sends the integer-keyed
//! write messages.

use crate::indexing::paths::app_text::native_name;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cmdr_fs::log_private::LogPrivate;
use rusqlite::Connection;

use crate::indexing::IndexPathSpace;
use crate::indexing::metadata::extract_metadata;
use crate::indexing::paths::path_prefix::compute_parent_path;
use crate::indexing::scanner;
use crate::indexing::store::IndexStore;
use crate::indexing::watch::watcher::FsChangeEvent;
use crate::indexing::writer::{IndexWriter, WriteMessage};

use super::dir_read::child_is_absent;
use super::escalation::resolve_escalation_anchor;
use super::throttle::ThrottleOutcome;
use super::{LiveThrottle, PendingUpsert};

/// Aggregator for high-volume reconciler skip/escalation events. Each per-event line
/// is at TRACE (off by default; file chain captures Debug+ only); this aggregator emits
/// a single DEBUG summary every ~60 s, so error report bundles still carry the
/// existence-of-drift signal without the per-event noise. Two instances cover the two
/// classes that dominate normal log volume: [`UNKNOWN_PATH_SKIPS`](skip_aggregator::UNKNOWN_PATH_SKIPS) (removal for a path
/// not in the DB) and [`ESCALATED_MISSING_PARENTS`](skip_aggregator::ESCALATED_MISSING_PARENTS) (create/modify whose parent dir
/// isn't in the DB — now escalated to a subtree rescan rather than dropped).
///
/// Most are harmless build-output churn, but a sustained rate (or a sample path in an
/// unexpected tree) can flag real reconciler/index drift. Triagers: if you need the
/// per-event detail, run with `RUST_LOG=cmdr_lib::indexing::reconciler=trace,debug`.
mod skip_aggregator {
    use std::sync::Mutex;
    use std::time::Instant;

    use cmdr_fs::pluralize::pluralize;

    /// One summary a minute, not one every five seconds. Both numbers on the line
    /// are rates over the window, so a wider window loses nothing and drops ~490
    /// lines an hour to ~40 on a build machine. The running `total` means a window
    /// that ends mid-count is picked up by the next one rather than lost.
    const FLUSH_INTERVAL_SECS: u64 = 60;
    const SAMPLE_LEN: usize = 80;

    struct State {
        count_since_last_flush: u64,
        total: u64,
        last_flush: Instant,
        /// One representative path from the current window. Truncated to SAMPLE_LEN
        /// chars so a verbose log dir doesn't blow up the line. Cleared on flush.
        sample: Option<String>,
    }

    /// One skip category: its own rolling state plus the words for the summary line
    /// (`"skipped {unit}s {reason} in …"`).
    pub(crate) struct SkipAggregator {
        state: Mutex<Option<State>>,
        /// Counted noun, e.g. `"removal"` → "skipped 3 removals".
        unit: &'static str,
        /// Reason phrase, e.g. `"for unknown paths"`.
        reason: &'static str,
    }

    impl SkipAggregator {
        const fn new(unit: &'static str, reason: &'static str) -> Self {
            Self {
                state: Mutex::new(None),
                unit,
                reason,
            }
        }

        /// Increment the counter and emit a summary if the flush interval has elapsed.
        /// Called on every skip (cheap: one mutex acquisition, one branch).
        pub(crate) fn record(&self, path: &str) {
            let mut guard = match self.state.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let state = guard.get_or_insert_with(|| State {
                count_since_last_flush: 0,
                total: 0,
                last_flush: Instant::now(),
                sample: None,
            });
            state.count_since_last_flush += 1;
            state.total += 1;
            if state.sample.is_none() {
                // Truncate on a CHAR boundary, not a byte index: NAS paths carry accented
                // names (e.g. "Külkeres síelés"), and `&path[..SAMPLE_LEN]` would panic
                // when byte SAMPLE_LEN lands mid-codepoint.
                let s = if path.chars().count() > SAMPLE_LEN {
                    format!("{}…", path.chars().take(SAMPLE_LEN).collect::<String>())
                } else {
                    path.to_string()
                };
                state.sample = Some(s);
            }
            if state.last_flush.elapsed().as_secs() >= FLUSH_INTERVAL_SECS {
                let count = state.count_since_last_flush;
                let total = state.total;
                let sample = state.sample.clone().unwrap_or_default();
                let secs = state.last_flush.elapsed().as_secs_f64();
                state.count_since_last_flush = 0;
                state.last_flush = Instant::now();
                state.sample = None;
                let (unit, reason) = (self.unit, self.reason);
                // Drop the lock before logging so the message format won't reenter under it.
                drop(guard);
                log::debug!(
                    "Reconciler: skipped {} {reason} in {secs:.1}s [{total} total], sample: {sample}",
                    pluralize(count, unit)
                );
            }
        }
    }

    /// Removals for a path that isn't in the DB (mostly harmless build-output churn).
    pub(crate) static UNKNOWN_PATH_SKIPS: SkipAggregator = SkipAggregator::new("removal", "for unknown paths");
    /// Create/modify events whose parent dir isn't in the DB: escalated to a subtree
    /// rescan of the highest missing dir (Leak B) rather than dropped.
    pub(crate) static ESCALATED_MISSING_PARENTS: SkipAggregator =
        SkipAggregator::new("event", "escalated for missing parents");
}

/// Process a single filesystem event. Returns the ORIGIN dirs it changed: the
/// event path's parent (whose listing changed), plus the path itself when it's a
/// newly created directory.
///
/// ❌ It does NOT return the ancestor chain. The chain is a different fact — "these
/// dirs' recursive sizes need refreshing" — that [`with_ancestor_closure`](crate::indexing::paths::path_prefix::with_ancestor_closure) rebuilds
/// where it's needed. Conflating the two once made the importance scheduler, which
/// expands each batch entry DOWNWARD into its subtree, rescore ~90,000 folders a
/// minute for a two-folder change (`indexing/lifecycle/DETAILS.md` § The lifecycle bus).
///
/// Shared between replay and live mode. Normalizes paths, checks exclusions,
/// stats the file, resolves paths to integer entry IDs, and sends appropriate
/// integer-keyed write messages (`UpsertEntryV2`, `DeleteEntryById`, etc.).
///
/// `throttle` is `Some` ONLY on the live path (`process_live_event`). Replay and
/// cold-start pass `None` so journal catch-up applies every event immediately.
/// When present, a regular file's in-place rewrite may be suppressed here (its
/// last-seen size flushed later by [`EventReconciler::sweep_throttle`](super::EventReconciler::sweep_throttle)).
///
/// `escalation` is the subtree this event leaves the index unable to vouch for: when
/// a create/modify event's parent chain is (partly) missing (Leak B), the rescan
/// anchor is the highest missing dir, instead of dropping the credit; when the event
/// creates a directory row, the anchor is that directory, whose contents nothing has
/// listed yet. The caller
/// (`process_live_event` live, buffered replay) owns the reconciler state and
/// queues or defers it. `None` means nothing to escalate. A typed `PathBuf`
/// out-param, never a string signal — no string-matching classification.
/// ⚠️ **Test-only.** Every production path can name the volume it is processing for,
/// so every one of them takes [`process_fs_event_into`] and holds its deletes until
/// the batch has read presence. This form sends them straight away, which is what the
/// per-event tests want: they assert on one event's writes with no batch around it.
/// ❌ Don't reach for it from production code — an ungated delete is exactly what the
/// batch gate exists to prevent.
#[cfg(test)]
pub(crate) fn process_fs_event(
    event: &FsChangeEvent,
    space: &IndexPathSpace,
    conn: &Connection,
    writer: &IndexWriter,
    throttle: Option<&mut LiveThrottle>,
    escalation: &mut Option<PathBuf>,
) -> Option<Vec<String>> {
    let mut deletes = Vec::new();
    let origins = process_fs_event_into(event, space, conn, writer, throttle, escalation, &mut deletes);
    for message in deletes {
        let _ = writer.send(message);
    }
    origins
}

/// The gathering form of the event path: it collects the deletes it decides on into
/// `deletes` instead of sending them. (`process_fs_event` is the test-only
/// immediate-send twin, so this doc can't link to it.)
///
/// ⚠️ The caller owns the gate: it sends the batch only after asking whether the
/// drive is still listed, ONCE for the batch and ❌ never per event. An event whose
/// stat failed because the drive went away looks exactly like a removal, so a batch
/// sent unconditionally is how a vanished drive deletes its own index.
#[allow(
    clippy::too_many_arguments,
    reason = "the event-processing param set plus the batch it gathers into; a struct would add indirection without clarity"
)]
pub(crate) fn process_fs_event_into(
    event: &FsChangeEvent,
    space: &IndexPathSpace,
    conn: &Connection,
    writer: &IndexWriter,
    throttle: Option<&mut LiveThrottle>,
    escalation: &mut Option<PathBuf>,
    deletes: &mut Vec<WriteMessage>,
) -> Option<Vec<String>> {
    // The canonical ABSOLUTE path in this volume's world. It stays absolute through
    // the whole function (FS stat, exclusion, the origin dirs, the FE emit);
    // the mount-relative strip is applied ONLY at each `resolve_abs` argument. For
    // the boot disk this firmlink-normalizes; for a mount-rooted drive it's the raw
    // path (firmlink semantics don't apply under `/Volumes`).
    let normalized = space.absolute(&event.path);

    // Skip excluded paths, scoped by the volume kind: `BootDisk` keeps the `/`-rooted
    // boot disk off `/Volumes/`/system trees; `MountRooted` skips only junk basenames
    // so an external drive still indexes its own subtree.
    if scanner::should_exclude(&normalized, space.exclusion_scope()) {
        return None;
    }

    // Skip HistoryDone marker events
    if event.flags.history_done {
        return None;
    }

    let parent_path = compute_parent_path(&normalized);
    // The ONE directory whose own listing this event changes. The ancestor chain
    // (which needs a recursive-size refresh, not a listing refresh) is rebuilt from
    // this by `path_prefix::with_ancestor_closure` at the batch's drain point.
    let mut origins: Vec<String> = origin_dir(&normalized).into_iter().collect();

    if event.flags.item_removed {
        return handle_removal(
            &normalized,
            space,
            conn,
            event,
            writer,
            origins,
            throttle,
            escalation,
            deletes,
        );
    }

    if event.flags.item_created || event.flags.item_modified || event.flags.item_renamed {
        return handle_creation_or_modification(
            &normalized,
            &parent_path,
            space,
            conn,
            event,
            writer,
            &mut origins,
            throttle,
            escalation,
            deletes,
        );
    }

    // For other flag combinations (xattr, owner change, etc.), just stat and update
    if event.flags.item_is_file || event.flags.item_is_dir {
        return handle_creation_or_modification(
            &normalized,
            &parent_path,
            space,
            conn,
            event,
            writer,
            &mut origins,
            throttle,
            escalation,
            deletes,
        );
    }

    None
}

/// Handle a file/directory removal event.
///
/// FSEvents can deliver `item_removed` for paths that still exist on disk
/// (e.g., atomic file swaps, coalesced events with OR'd flags). To avoid
/// deleting live entries, we stat the path first: if it exists, delegate to
/// `handle_creation_or_modification` (which upserts). Only delete from the DB
/// when the path is truly gone from the filesystem.
#[allow(
    clippy::too_many_arguments,
    reason = "shares the event-processing param set; a struct would add indirection without clarity"
)]
fn handle_removal(
    normalized: &str,
    space: &IndexPathSpace,
    conn: &Connection,
    event: &FsChangeEvent,
    writer: &IndexWriter,
    mut origins: Vec<String>,
    throttle: Option<&mut LiveThrottle>,
    escalation: &mut Option<PathBuf>,
    deletes: &mut Vec<WriteMessage>,
) -> Option<Vec<String>> {
    // Check if the path actually exists on disk before deleting from the DB.
    // `normalized` is the absolute FS path, so this stat is correct on any volume.
    match Path::new(normalized).symlink_metadata() {
        Ok(_) => {
            // Path still exists, so treat as a modification, not a removal (throttled
            // like any other in-place rewrite). Deletes themselves are never throttled.
            let parent_path = compute_parent_path(normalized);
            return handle_creation_or_modification(
                normalized,
                &parent_path,
                space,
                conn,
                event,
                writer,
                &mut origins,
                throttle,
                escalation,
                deletes,
            );
        }
        // Really gone: the one observation that earns a delete.
        Err(e) if child_is_absent(&e) => {}
        // We never got to look — a permission wall, an I/O error, a drive on its way
        // out. ❌ Not evidence of a removal, so the row stays and a later pass heals it.
        Err(_) => return Some(origins),
    }

    // Path is truly gone; resolve (mount-strip for a mount-rooted drive) and delete.
    let entry_id = match space.resolve_abs(conn, normalized) {
        Ok(Some(id)) => id,
        Ok(None) => {
            // Per-event line at TRACE: useful when actively debugging reconciler/index
            // drift, but ~90% of normal log volume comes from build-output churn that's
            // genuinely harmless. The aggregate at DEBUG (below) gives the existence-of-
            // drift signal without flooding the file.
            log::trace!("Reconciler: removal for unknown path, skipping: {normalized}");
            skip_aggregator::UNKNOWN_PATH_SKIPS.record(normalized);
            return Some(origins);
        }
        Err(e) => {
            log::warn!("Reconciler: resolve_path failed for removal {normalized}: {e}");
            return Some(origins);
        }
    };

    deletes.push(delete_for(conn, entry_id, event));

    Some(origins)
}

/// The delete for a row whose path is gone: its whole subtree when the event says it
/// was a folder OR the row is one.
///
/// ⚠️ The row is what knows. A removal reports a path that's already gone, so
/// nothing can stat its type, and `ReadDirectoryChangesW` (`FILE_ACTION_REMOVED`,
/// `FILE_ACTION_RENAMED_OLD_NAME`) and inotify's `notify` translation carry none:
/// trusting the flag alone deleted just a folder's row, orphaning its children and
/// leaving their bytes in every ancestor (#1363). A read that fails falls back to the
/// flag.
fn delete_for(conn: &Connection, id: i64, event: &FsChangeEvent) -> WriteMessage {
    let is_dir = event.flags.item_is_dir
        || matches!(IndexStore::get_entry_by_id(conn, id), Ok(Some(entry)) if entry.is_directory);
    if is_dir {
        WriteMessage::DeleteSubtreeById(id)
    } else {
        WriteMessage::DeleteEntryById(id)
    }
}

/// Handle file/directory creation, modification, or rename.
///
/// Resolves the parent path to an integer ID and sends `UpsertEntryV2`.
/// For new entries (create), also sends `PropagateDeltaById` starting
/// from the parent so dir_stats are updated along the ancestor chain.
#[allow(
    clippy::too_many_arguments,
    reason = "shares the event-processing param set (path/parent/space/conn/event/writer/origins/throttle/escalation); a struct would add indirection without clarity, matching run_scan"
)]
fn handle_creation_or_modification(
    normalized: &str,
    parent_path: &str,
    space: &IndexPathSpace,
    conn: &Connection,
    event: &FsChangeEvent,
    writer: &IndexWriter,
    origins: &mut Vec<String>,
    throttle: Option<&mut LiveThrottle>,
    escalation: &mut Option<PathBuf>,
    deletes: &mut Vec<WriteMessage>,
) -> Option<Vec<String>> {
    // Stat the file to get current metadata. `normalized` is the absolute FS path.
    let path = Path::new(normalized);
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        // We never got to look at it: ❌ not evidence that it went away.
        Err(e) if !child_is_absent(&e) => return Some(origins.clone()),
        Err(_) => {
            // Path doesn't exist (deleted since event was generated).
            // Treat as a removal: resolve to entry ID and gather an integer-keyed delete.
            // Use DeleteSubtreeById for directories to also remove child entries;
            // journal replay may coalesce child events into a parent dir event,
            // leaving orphaned children without a subtree delete.
            match space.resolve_abs(conn, normalized) {
                Ok(Some(id)) => deletes.push(delete_for(conn, id, event)),
                Ok(None) => {
                    // Not in DB either -- nothing to do
                }
                Err(e) => {
                    log::warn!("Reconciler: resolve_path failed for gone path {normalized}: {e}");
                }
            }
            return Some(origins.clone());
        }
    };

    // Resolve parent path to integer ID (mount-strip for a mount-rooted drive).
    let parent_id = match space.resolve_abs(conn, parent_path) {
        Ok(Some(id)) => id,
        Ok(None) => {
            // Parent not in DB (Leak B): the intermediate dir chain is missing.
            // Instead of dropping the credit, escalate to a subtree rescan anchored
            // at the highest missing dir, so `reconcile_subtree` discovers and
            // credits the whole chain. The caller queues (live) or defers (replay).
            // Per-event at TRACE; a DEBUG aggregate every ~5 s keeps the drift
            // signal in error reports without the per-event flood.
            if let Some(anchor) = resolve_escalation_anchor(space, conn, normalized) {
                *escalation = Some(anchor);
            }
            log::trace!(
                "Reconciler: parent path not in DB, escalating event for {} (parent: {})",
                LogPrivate::path(normalized),
                LogPrivate::dir_path(parent_path)
            );
            skip_aggregator::ESCALATED_MISSING_PARENTS.record(normalized);
            return Some(origins.clone());
        }
        Err(e) => {
            log::warn!(
                "Reconciler: resolve_path failed for parent {}: {e}",
                LogPrivate::dir_path(parent_path)
            );
            return Some(origins.clone());
        }
    };

    let is_dir = metadata.is_dir();
    let is_symlink = metadata.is_symlink();
    let name = path
        .file_name()
        .map(|n| native_name(n).into_owned())
        .unwrap_or_default();

    let snap = extract_metadata(&metadata, is_dir, is_symlink);
    // On a volume without stable inodes (FAT/exFAT) store `inode: None`, so a
    // reused inode can never let the live rename pre-pass false-match a move.
    let inode = space.trust_inode(snap.inode);

    // Live-path throttle: a regular file rewritten in place may be suppressed so
    // rapid rewrites collapse to ≤1 index write per THROTTLE_WINDOW. Only files
    // (dirs/symlinks carry no size), never on replay (`throttle` is None), and
    // never under the user's Downloads (active downloads want a live size). The
    // trailing flush that applies the suppressed size runs from the sweep tick.
    let is_regular_file = !is_dir && !is_symlink;
    let suppress = match throttle {
        Some(t) if is_regular_file && !t.is_exempt(normalized) => {
            let payload = PendingUpsert {
                parent_id,
                name: name.clone(),
                logical_size: snap.logical_size,
                physical_size: snap.physical_size,
                modified_at: snap.modified_at,
                inode,
                nlink: snap.nlink,
            };
            matches!(
                t.on_change(normalized, snap.logical_size.unwrap_or(0), payload, Instant::now()),
                ThrottleOutcome::Suppress
            )
        }
        _ => false,
    };

    if suppress {
        // Nothing written, so no dir_stats changed: don't notify ancestors. The
        // last-seen size is applied by the trailing-flush sweep.
        return Some(Vec::new());
    }

    // A directory row this upsert CREATES lands unlisted (`listed_epoch = 0`): the
    // event says the folder is there, not what's in it (a folder moved in brings
    // contents nobody reported). Queue its subtree so a walk lists it and lifts its
    // coverage; without one it and every ancestor read "≥" until the next launch
    // whenever its contents arrive in later batches, under a parent that resolves,
    // where nothing escalates. The settle delay holds a brand-new anchor back, so
    // short-lived build trees walk to a clean no-op.
    if is_dir && !holds_directory_row(conn, parent_id, &name) {
        *escalation = Some(path.to_path_buf());
    }

    let _ = writer.send(WriteMessage::UpsertEntryV2 {
        parent_id,
        name,
        is_directory: is_dir,
        is_symlink,
        logical_size: snap.logical_size,
        physical_size: snap.physical_size,
        modified_at: snap.modified_at,
        inode,
        nlink: snap.nlink,
    });

    // UpsertEntryV2 auto-propagates deltas in the writer, so no separate
    // PropagateDeltaById needed here.

    // A newly created directory is an origin in its own right: it appeared, so its
    // (empty, or about-to-be-scanned) subtree is new to every downstream consumer.
    if event.flags.item_created && is_dir {
        origins.push(normalized.to_string());
    }

    Some(origins.clone())
}

/// Whether `parent_id` already holds a DIRECTORY row named `name`. A miss, a
/// file row (a file→dir type change), or a failed read all answer `false`, so the
/// caller queues a walk: one walk too many costs a listing, one too few leaves the
/// folder unlisted.
fn holds_directory_row(conn: &Connection, parent_id: i64, name: &str) -> bool {
    let Ok(Some(id)) = IndexStore::resolve_component(conn, parent_id, name) else {
        return false;
    };
    matches!(IndexStore::get_entry_by_id(conn, id), Ok(Some(entry)) if entry.is_directory)
}

/// The directory whose OWN listing a change at `path` alters: its immediate
/// parent. `None` when there is none (the root itself, or a relative path).
///
/// This is the "origin" half of the two facts a live change carries. The other
/// half — every ancestor whose recursive SIZE now needs refreshing — is rebuilt
/// from the origins by [`with_ancestor_closure`](crate::indexing::paths::path_prefix::with_ancestor_closure). Keeping them apart is what stops
/// a consumer that expands DOWNWARD (the importance scheduler's incremental
/// rescore) from seeing `/Users` in every batch; see
/// `indexing/lifecycle/DETAILS.md` § The lifecycle bus.
pub(super) fn origin_dir(path: &str) -> Option<String> {
    let parent = compute_parent_path(path);
    if parent.is_empty() || parent == path {
        return None;
    }
    Some(parent)
}
