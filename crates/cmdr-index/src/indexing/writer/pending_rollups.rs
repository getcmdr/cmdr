//! The routine ancestor roll-up queue: one repair per BURST of subtree
//! aggregates, not one per subtree.
//!
//! A finished frontier-root walk ends in `ComputeSubtreeAggregates`, whose
//! handler owes the subtree root's ancestors a recompute. Doing that inline made
//! covering a wide directory quadratic: a stopped walk turns every unwalked
//! child of a `W`-child directory into a frontier root of its own, and each of
//! the `W` roots then recomputed that one parent from all `W` of its children
//! (`docs/notes/wide-dir-scaling-2026-08-18.md`).
//!
//! So the handler queues the ancestor here instead, and `writer_loop` drains the
//! queue at its caught-up point. The queue is a SET, so a burst of `W` roots
//! sharing one parent costs one roll-up rather than `W`.
//!
//! **Why deferring is safe.** [`super::repair::repair_dir_stats_upward`]
//! recomputes each level from its committed children: it writes an ABSOLUTE
//! value, never a relative one, so it can't double-count whatever landed while
//! it waited, and running it later can only make it see MORE of the truth. It is
//! idempotent and order-independent for the same reason. Every writer of this
//! DB is this one thread, so nothing can interleave a write between the queue
//! and the drain. See `DETAILS.md` § "The dir_stats ledger" for the full
//! argument, including what reads the stale row in between.
//!
//! **❌ Not [`super::repair::DeferredRepairs`].** That queue is drift
//! TELEMETRY: it warns on its first entry, caps at 1,024 ids, and gives up on an
//! id after five attempts. All three are wrong here — this is the routine path,
//! so the warning would fire on every first index, and a dropped roll-up is a
//! permanently wrong size rather than a missed retry.
//!
//! **❌ The trigger stays the caught-up point** — never a timer, a size cap, or
//! "every N messages". An empty queue is what makes a recompute-from-children see
//! final rows, and it is also what makes the coalescing self-limiting: the writer
//! only pays for a roll-up when it has nothing else to do, so a burst costs about
//! what the work that filled it cost. The drain's slice (`DRAIN_SLICE`) only ever
//! ENDS a drain early for a waiting message; it never starts one.
//!
//! **❌ The drain never walks each queued chain to the root on its own.** Chains
//! share ancestors, and a shared wide one recomputed once per chain is
//! `O(width²)` one level up (GitHub issue #1480). [`PendingRollups::drain`]
//! recomputes each owed directory once, deepest first, and keeps its heap across
//! a yield, so a resumed drain re-reads nothing it already knew.
//!
//! **❌ A queued id is never dropped because its chain can't be read.** It stays
//! owed (recomputed last), and a parent whose read fails is retried at the next
//! drain: a dropped id is a wrong size for good.
//!
//! Writer-thread only, like the queue next door: created in `writer_loop`,
//! threaded through the handlers, drained on the same thread, so interior
//! mutability needs no locking.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cmdr_fs::pluralize::pluralize;

use crate::indexing::IndexFailureSignal;
use crate::indexing::events::{EventSink, emit_dir_updated};
use crate::indexing::store::{IndexStore, IndexStoreError, ROOT_ID};

use super::delta::ChainSteps;
use super::repair::{DeferredRepairs, LevelRepair, repair_one_level};

/// How long a roll-up drain recomputes before it checks whether a message is
/// waiting. Matches the implicit batch's cap (`batch.rs`): a live change waits
/// about this long, at most, behind a drain. The clock starts AFTER the drain has
/// read the chains of newly queued ids, so every slice buys a slice of recomputes.
pub(super) const DRAIN_SLICE: Duration = Duration::from_millis(250);

/// How long roll-ups may stay owed before the writer says so at `warn`: past
/// this, live work is taking the writer often enough that sizes aren't settling.
const OWED_TOO_LONG: Duration = Duration::from_secs(60);

/// The roll-up queue's numbers, shared with `IndexWriter` (tests, the heartbeat)
/// because those read them from another thread.
#[derive(Default)]
pub(crate) struct RollupGauges {
    /// Single-directory recomputes run so far. Against the frontier roots covered,
    /// the scaling guards' number: one per directory that needed one is linear.
    pub(crate) recomputes: AtomicU64,
    /// `get_parent_id` reads the drains made. Linear in the directories owed, or a
    /// yield is re-reading what it already knew.
    pub(crate) parent_reads: AtomicU64,
    /// Drains that gave the writer back with roll-ups still owed.
    pub(crate) yields: AtomicU64,
    /// Directories owed right now (queued, in the heap, or waiting on a parent
    /// read), for the heartbeat.
    pub(crate) owed: AtomicU64,
    /// The drain slice in nanoseconds; `0` means [`DRAIN_SLICE`]. Only a test sets it.
    slice_nanos: AtomicU64,
    /// A test's "a message is always waiting", so every drain yields after its slice.
    #[cfg(test)]
    pub(crate) yield_always: std::sync::atomic::AtomicBool,
    /// How many drain commits a test wants to fail (as `SQLITE_BUSY`, which the
    /// failure signal counts but doesn't stop the writer for).
    #[cfg(test)]
    pub(crate) fail_commits: AtomicU64,
}

impl RollupGauges {
    fn slice(&self) -> Duration {
        match self.slice_nanos.load(Ordering::Relaxed) {
            0 => DRAIN_SLICE,
            nanos => Duration::from_nanos(nanos),
        }
    }

    /// Shrink the drain slice, so a test can make every drain yield.
    #[cfg(test)]
    pub(crate) fn set_slice_for_test(&self, slice: Duration) {
        self.slice_nanos
            .store(slice.as_nanos().max(1) as u64, Ordering::Relaxed);
    }

    fn yields_always(&self) -> bool {
        #[cfg(test)]
        return self.yield_always.load(Ordering::Relaxed);
        #[cfg(not(test))]
        false
    }

    /// A commit failure a test injected, if one is still due.
    fn injected_commit_failure(&self) -> Option<IndexStoreError> {
        #[cfg(test)]
        if self
            .fail_commits
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
            .is_ok()
        {
            return Some(IndexStoreError::from(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                Some("injected roll-up commit failure".to_string()),
            )));
        }
        None
    }
}

/// Ancestors a finished subtree aggregate owes a roll-up, waiting for the
/// writer's caught-up point.
///
/// Deduped and UNBOUNDED: one entry per distinct directory, which is what makes
/// the wide-directory burst collapse to a single recompute, and a few bytes an id
/// even when a whole phase's parents pile up. ❌ Don't cap it — an id dropped here
/// is a directory whose size stays wrong until a full aggregate rebuilds the volume.
pub(super) struct PendingRollups {
    volume_id: String,
    /// Newly queued ids whose depth the next drain reads.
    queued: RefCell<BTreeSet<i64>>,
    /// What a yield left owed, kept across drains so a resumed drain re-reads nothing.
    work: RefCell<Work>,
    gauges: Arc<RollupGauges>,
    /// When the current stretch of owed work started, and when it last warned.
    owed_since: Cell<Option<Instant>>,
    last_owed_warning: Cell<Option<Instant>>,
    drains_this_stretch: Cell<u64>,
    /// A test's injected `get_parent_id` failure for one id.
    #[cfg(test)]
    fail_parent_read_of: Cell<Option<i64>>,
}

/// The owed directories, deepest first, plus what's known about their depths.
#[derive(Default)]
struct Work {
    /// `(depth, id)`, a max-heap so the deepest directory pops first.
    heap: BinaryHeap<(usize, i64)>,
    in_heap: HashSet<i64>,
    /// Rewritten directories whose parent read failed: the parent is owed, but
    /// which one isn't known yet. Retried at the next drain.
    owes_parent: BTreeSet<i64>,
    /// Depth of every directory a chain read reached. Only an ORDER: a stale depth
    /// after a move costs a second recompute, never a wrong size, because every
    /// rewrite owes the parent read fresh.
    depths: HashMap<i64, usize>,
    /// Every id this drain recomputed, so a drain whose transaction doesn't commit
    /// can owe them all again.
    this_drain: Vec<i64>,
}

impl Work {
    fn owe(&mut self, id: i64, depth: usize) {
        if self.in_heap.insert(id) {
            self.heap.push((depth, id));
        }
    }

    fn is_empty(&self) -> bool {
        self.heap.is_empty() && self.owes_parent.is_empty()
    }
}

impl PendingRollups {
    pub(super) fn new(volume_id: &str, gauges: Arc<RollupGauges>) -> Self {
        Self {
            volume_id: volume_id.to_string(),
            queued: RefCell::new(BTreeSet::new()),
            work: RefCell::new(Work::default()),
            gauges,
            owed_since: Cell::new(None),
            last_owed_warning: Cell::new(None),
            drains_this_stretch: Cell::new(0),
            #[cfg(test)]
            fail_parent_read_of: Cell::new(None),
        }
    }

    /// Remember that `id`'s chain owes a roll-up. Cheap and silent: this fires
    /// once per frontier root a first index covers.
    pub(super) fn queue(&self, id: i64) {
        self.queued.borrow_mut().insert(id);
        self.publish_owed();
    }

    /// Forget everything owed, because the rows it names are gone. ONLY
    /// `TruncateData`: an id dropped for any other reason is a permanently wrong
    /// size. The depths go too, since a truncate resets the id counter.
    pub(super) fn clear(&self) {
        self.queued.borrow_mut().clear();
        *self.work.borrow_mut() = Work::default();
        self.publish_owed();
    }

    /// Whether any roll-up is still owed.
    pub(super) fn is_empty(&self) -> bool {
        self.queued.borrow().is_empty() && self.work.borrow().is_empty()
    }

    fn owed_count(&self) -> usize {
        let work = self.work.borrow();
        self.queued.borrow().len() + work.heap.len() + work.owes_parent.len()
    }

    fn publish_owed(&self) {
        self.gauges.owed.store(self.owed_count() as u64, Ordering::Relaxed);
    }

    /// One `get_parent_id`, counted. A test can make it fail for one id.
    fn read_parent(&self, conn: &rusqlite::Connection, id: i64) -> Result<Option<i64>, IndexStoreError> {
        self.gauges.parent_reads.fetch_add(1, Ordering::Relaxed);
        #[cfg(test)]
        if self.fail_parent_read_of.get() == Some(id) {
            return Err(IndexStoreError::Io(std::io::Error::other(
                "injected parent read failure",
            )));
        }
        IndexStore::get_parent_id(conn, id)
    }

    /// `id`'s depth: walk up until a directory whose depth is known, or the top.
    /// A chain that can't be read (a failed read, a missing ancestor, a loop,
    /// which `ChainSteps` also marks for a rebuild) still leaves `id` owed: it
    /// gets depth 0, so it's recomputed last, and its rewrite reads the parent
    /// fresh like any other.
    fn depth_of(&self, conn: &rusqlite::Connection, work: &mut Work, id: i64) -> usize {
        let mut climbed = Vec::new();
        let mut steps = ChainSteps::new("ancestor roll-up", id);
        let mut current = id;
        let base = loop {
            if let Some(&depth) = work.depths.get(&current) {
                break depth;
            }
            if current == ROOT_ID {
                work.depths.insert(ROOT_ID, 0);
                break 0;
            }
            if !steps.another(conn) {
                return 0;
            }
            match self.read_parent(conn, current) {
                Ok(Some(parent)) if parent != 0 => {
                    climbed.push(current);
                    current = parent;
                }
                // A directory directly under nothing is a top of its own.
                Ok(Some(_)) => {
                    work.depths.insert(current, 0);
                    break 0;
                }
                Ok(None) => return 0,
                Err(e) => {
                    log::debug!(target: "indexing::writer", "ancestor roll-up: parent read failed for id={current} (on the chain of id={id}): {e}");
                    return 0;
                }
            }
        };
        let mut depth = base;
        for &dir in climbed.iter().rev() {
            depth += 1;
            work.depths.insert(dir, depth);
        }
        depth
    }

    /// Owe `id`'s parent, read fresh: a cached parent goes stale when something
    /// moves the directory, and a stale one would skip the new parent.
    fn owe_parent_of(&self, conn: &rusqlite::Connection, work: &mut Work, id: i64, depth: usize) {
        if id == ROOT_ID {
            return;
        }
        match self.read_parent(conn, id) {
            Ok(Some(parent)) if parent != 0 => {
                let parent_depth = work.depths.get(&parent).copied().unwrap_or(depth.saturating_sub(1));
                work.owe(parent, parent_depth);
            }
            Ok(_) => {}
            Err(e) => {
                log::debug!(target: "indexing::writer", "ancestor roll-up: parent read failed for id={id}: {e}; retried at the next drain");
                work.owes_parent.insert(id);
            }
        }
    }

    /// Roll the owed ancestors up, deepest first, each directory ONCE.
    ///
    /// Walking each queued id's chain to the root on its own recomputed every
    /// ancestor the chains SHARE once per chain: `W` queued parents under one
    /// `W`-child grandparent recomputed that grandparent `W` times, each a `SUM`
    /// over `W` children (GitHub issue #1480, over an hour on a Windows `C:`). So
    /// the owed directories sit in a max-heap by depth: a rewrite owes the parent
    /// (read fresh), an agreeing row owes nothing (the per-chain short-circuit,
    /// kept), and a directory several children owe is in the heap once.
    ///
    /// Gives the writer back when `wants_the_writer` says a message is waiting
    /// and the drain has recomputed for a whole slice ([`DRAIN_SLICE`]). The heap
    /// and the depths survive the yield, so the next drain reads only the chains
    /// of ids queued since: a yield costs nothing it has already paid for. Safe
    /// for the same reason deferring is (each recompute is absolute). Exit paths
    /// pass a closure that never yields.
    pub(super) fn drain(
        &self,
        conn: &rusqlite::Connection,
        repairs: &DeferredRepairs,
        wants_the_writer: &dyn Fn() -> bool,
    ) -> Drained {
        if self.is_empty() {
            return Drained::default();
        }
        let started = Instant::now();
        let mut work = self.work.borrow_mut();
        work.this_drain.clear();

        // Newly queued ids, each chain read once, shared prefixes once. Then the
        // parents an earlier drain couldn't read.
        let queued = std::mem::take(&mut *self.queued.borrow_mut());
        let newly = queued.len();
        for id in queued {
            let depth = self.depth_of(conn, &mut work, id);
            work.owe(id, depth);
        }
        for id in std::mem::take(&mut work.owes_parent) {
            let depth = work.depths.get(&id).copied().unwrap_or(0);
            self.owe_parent_of(conn, &mut work, id, depth);
        }
        let chain_read = started.elapsed();

        let slice_started = Instant::now();
        let slice = self.gauges.slice();
        let mut result = Drained::default();
        let mut recomputes = 0u64;
        while let Some((depth, id)) = work.heap.pop() {
            work.in_heap.remove(&id);
            work.this_drain.push(id);
            recomputes += 1;
            if repair_one_level(conn, id, repairs) == LevelRepair::Rewrote {
                result.moved = true;
                self.owe_parent_of(conn, &mut work, id, depth);
            }
            if !work.heap.is_empty()
                && slice_started.elapsed() >= slice
                && (wants_the_writer() || self.gauges.yields_always())
            {
                result.owes_more = true;
                break;
            }
        }
        if work.is_empty() {
            // Nothing owed: the depths would only go stale.
            work.depths.clear();
        }
        let left = work.heap.len() + work.owes_parent.len();
        drop(work);
        self.gauges.recomputes.fetch_add(recomputes, Ordering::Relaxed);
        if result.owes_more {
            self.gauges.yields.fetch_add(1, Ordering::Relaxed);
        }
        self.publish_owed();
        log::debug!(
            target: "indexing::writer",
            "Ancestor roll-up: {} newly queued, chains read in {}ms, {} in {}ms{}, {} left",
            pluralize(newly as u64, "directory"),
            chain_read.as_millis(),
            pluralize(recomputes, "recompute"),
            slice_started.elapsed().as_millis(),
            if result.moved { "" } else { " (nothing moved)" },
            pluralize(left as u64, "directory"),
        );
        result
    }

    /// The drain's transaction didn't commit, so nothing it recomputed landed:
    /// owe all of it again.
    fn owe_last_drain_again(&self) {
        let mut work = self.work.borrow_mut();
        for id in std::mem::take(&mut work.this_drain) {
            let depth = work.depths.get(&id).copied().unwrap_or(0);
            work.owe(id, depth);
        }
        drop(work);
        self.publish_owed();
    }

    /// Track how long roll-ups have stayed owed, and say so at `warn` once a
    /// minute while they outlive [`OWED_TOO_LONG`]: live work taking the writer
    /// before sizes settle is otherwise visible only at `debug`.
    fn note_stretch(&self) {
        if self.is_empty() {
            self.owed_since.set(None);
            self.last_owed_warning.set(None);
            self.drains_this_stretch.set(0);
            return;
        }
        let now = Instant::now();
        let since = *self.owed_since.get().get_or_insert(now);
        self.owed_since.set(Some(since));
        self.drains_this_stretch.set(self.drains_this_stretch.get() + 1);
        let owed_for = now - since;
        let due = self.last_owed_warning.get().is_none_or(|at| now - at >= OWED_TOO_LONG);
        if owed_for >= OWED_TOO_LONG && due {
            self.last_owed_warning.set(Some(now));
            log::warn!(
                "Writer: '{}' still owes {} an ancestor roll-up after {} s and {}; live work keeps taking the writer, so some folder sizes are short until it settles",
                self.volume_id,
                pluralize(self.owed_count() as u64, "directory"),
                owed_for.as_secs(),
                pluralize(self.drains_this_stretch.get(), "drain"),
            );
        }
    }
}

/// What one [`PendingRollups::drain`] did.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Drained {
    /// A `dir_stats` row changed, so the panes owe a refresh.
    pub(super) moved: bool,
    /// The drain gave the writer back, or didn't commit, with roll-ups still owed.
    pub(super) owes_more: bool,
}

/// Settle the `dir_stats` ledger: roll up the ancestors a burst of subtree
/// aggregates left owing, then repair whatever chains a failed propagation queued.
///
/// Both want the writer's caught-up point (`DETAILS.md` § "The caught-up point")
/// and both are idempotent, so the exit paths call this again on the way out.
/// `is_autocommit()` keeps them out of an open `BeginTransaction` batch, where the
/// tree is only half written: rolling ancestors up from a partial state and then
/// dropping the id would bake that half-state in.
///
/// The roll-ups run inside ONE `BEGIN IMMEDIATE`, so a drain of thousands of
/// directories pays one commit instead of one per rewritten row (the 31 µs vs
/// 7 µs per row `batch.rs` measured). If that transaction doesn't commit, the
/// rows roll back, so everything the drain recomputed is owed again, and the
/// commit's error goes through `signal` like every other writer error: a full
/// disk pauses the index, a dead one fails it.
///
/// `wants_the_writer` lets the roll-up give the writer back to a waiting message
/// (see [`PendingRollups::drain`]); the deferred repairs, which want FINAL rows,
/// then wait for the next caught-up point too.
///
/// Returns what moved and whether anything is still owed, so a caller can refresh
/// the panes exactly when there is something to see, and claim "settled" only
/// when it is.
pub(super) fn settle_the_ledger(
    conn: &rusqlite::Connection,
    rollups: &PendingRollups,
    repairs: &DeferredRepairs,
    signal: &IndexFailureSignal,
    wants_the_writer: &dyn Fn() -> bool,
) -> Drained {
    if !conn.is_autocommit() {
        return Drained::default();
    }
    let mut drained = Drained::default();
    if !rollups.is_empty() {
        let in_transaction = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
        drained = rollups.drain(conn, repairs, wants_the_writer);
        if in_transaction && let Err(e) = commit_the_drain(conn, &rollups.gauges) {
            signal.note(&e, "ancestor roll-up commit");
            rollups.owe_last_drain_again();
            drained = Drained {
                moved: false,
                owes_more: true,
            };
        }
        rollups.note_stretch();
    }
    if !drained.owes_more && !repairs.is_empty() {
        repairs.drain(conn);
    }
    drained
}

/// Commit the drain's transaction. A fatal error mid-drain can roll it back on
/// its own, and a failed `COMMIT` leaves it open: either way none of it landed.
fn commit_the_drain(conn: &rusqlite::Connection, gauges: &RollupGauges) -> Result<(), IndexStoreError> {
    if let Some(e) = gauges.injected_commit_failure() {
        let _ = conn.execute_batch("ROLLBACK");
        return Err(e);
    }
    if conn.is_autocommit() {
        return Err(IndexStoreError::Io(std::io::Error::other(
            "the roll-up's transaction rolled back before its commit",
        )));
    }
    if let Err(e) = conn.execute_batch("COMMIT") {
        if !conn.is_autocommit() {
            let _ = conn.execute_batch("ROLLBACK");
        }
        return Err(e.into());
    }
    Ok(())
}

/// Tell both panes to re-read their sizes, from BETWEEN messages rather than from
/// inside a handler. The pool is what `process_message` gets for free: on macOS an
/// `emit` can autorelease ObjC objects on this background thread, and out here
/// there is no pool to catch them.
pub(super) fn emit_full_refresh(events: &dyn EventSink) {
    #[cfg(target_os = "macos")]
    objc2::rc::autoreleasepool(|_| emit_dir_updated(events, vec!["/".to_string()]));
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    emit_dir_updated(events, vec!["/".to_string()]);
}

#[cfg(test)]
mod tests;
