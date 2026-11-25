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
//! recomputes each owed directory once, deepest first.
//!
//! Writer-thread only, like the queue next door: created in `writer_loop`,
//! threaded through the handlers, drained on the same thread, so interior
//! mutability needs no locking.

use std::cell::RefCell;
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cmdr_fs::pluralize::pluralize;

use crate::indexing::events::{EventSink, emit_dir_updated};
use crate::indexing::store::{IndexStore, ROOT_ID};

use super::delta::ChainSteps;
use super::repair::{DeferredRepairs, LevelRepair, repair_one_level};

/// Ancestors a finished subtree aggregate owes a roll-up, waiting for the
/// writer's caught-up point.
///
/// Deduped and UNBOUNDED: one entry per distinct parent, which is what makes the
/// wide-directory burst collapse to a single walk, and 8 bytes an id even when a
/// whole phase's parents pile up. ❌ Don't cap it — an id dropped here is a
/// directory whose size stays wrong until a full aggregate rebuilds the volume.
pub(super) struct PendingRollups {
    pending: RefCell<BTreeSet<i64>>,
    /// How many single-directory recomputes this writer's drains have run. Against
    /// the number of frontier roots covered, this is the scaling guards' number:
    /// one per directory that needed one means linear. Shared with `IndexWriter`
    /// because the guards run on another thread.
    recomputes: Arc<AtomicU64>,
}

impl PendingRollups {
    pub(super) fn new(recomputes: Arc<AtomicU64>) -> Self {
        Self {
            pending: RefCell::new(BTreeSet::new()),
            recomputes,
        }
    }

    /// Remember that `id`'s chain owes a roll-up. Cheap and silent: this fires
    /// once per frontier root a first index covers.
    pub(super) fn queue(&self, id: i64) {
        self.pending.borrow_mut().insert(id);
    }

    /// Forget everything queued, because the rows it names are gone. ONLY
    /// `TruncateData`: an id dropped for any other reason is a permanently wrong
    /// size.
    pub(super) fn clear(&self) {
        self.pending.borrow_mut().clear();
    }

    /// Whether a roll-up is still owed.
    pub(super) fn is_empty(&self) -> bool {
        self.pending.borrow().is_empty()
    }

    /// Roll the queued ancestors up, deepest first, each directory ONCE.
    ///
    /// Walking each queued id's chain to the root on its own recomputed every
    /// ancestor the chains SHARE once per chain: `W` queued parents under one
    /// `W`-child grandparent recomputed that grandparent `W` times, each a `SUM`
    /// over `W` children (GitHub issue #1480, over an hour on a Windows `C:`). So
    /// the drain orders the owed directories by depth and recomputes one only
    /// after every owed directory below it: a rewrite owes the parent, an
    /// agreeing row owes nothing (the per-chain short-circuit, kept), and a
    /// parent several chains owe is still recomputed once. The cost is one
    /// recompute per directory that actually needs one.
    ///
    /// Gives the writer back when `wants_the_writer` says a message is waiting
    /// and the drain has run for `slice` ([`DRAIN_SLICE`] in the writer): what's
    /// still owed goes back in the set for the next caught-up point, so a live
    /// change never waits behind a long drain. Safe for the same reason deferring
    /// is (each recompute is absolute). Exit paths pass a closure that never yields.
    pub(super) fn drain(
        &self,
        conn: &rusqlite::Connection,
        repairs: &DeferredRepairs,
        slice: Duration,
        wants_the_writer: &dyn Fn() -> bool,
    ) -> Drained {
        let ids = std::mem::take(&mut *self.pending.borrow_mut());
        if ids.is_empty() {
            return Drained::default();
        }
        let t = Instant::now();
        let queued = ids.len();

        // Every queued id's chain up to the root, shared prefixes walked once.
        let mut chains = Chains::default();
        let mut owed = BinaryHeap::new();
        for &id in &ids {
            if let Some(depth) = chains.depth_of(conn, id, repairs) {
                owed.push((depth, id));
            }
        }
        // A directory can be owed by several children; it's in `owed` once.
        let mut in_owed: HashSet<i64> = owed.iter().map(|&(_, id)| id).collect();

        let mut result = Drained::default();
        let mut recomputes = 0u64;
        while let Some((depth, id)) = owed.pop() {
            in_owed.remove(&id);
            recomputes += 1;
            if repair_one_level(conn, id, repairs) == LevelRepair::Rewrote {
                result.moved = true;
                if let Some(parent) = chains.parent_of(id)
                    && in_owed.insert(parent)
                {
                    owed.push((depth - 1, parent));
                }
            }
            if !owed.is_empty() && t.elapsed() >= slice && wants_the_writer() {
                let mut pending = self.pending.borrow_mut();
                pending.extend(owed.drain().map(|(_, id)| id));
                result.owes_more = true;
                break;
            }
        }
        self.recomputes.fetch_add(recomputes, Ordering::Relaxed);
        log::debug!(
            target: "indexing::writer",
            "Ancestor roll-up: {} queued, {} in {}ms{}{}",
            pluralize(queued as u64, "ancestor"),
            pluralize(recomputes, "recompute"),
            t.elapsed().as_millis(),
            if result.moved { "" } else { " (nothing moved)" },
            if result.owes_more {
                format!(", {} left for the next quiet moment", pluralize(self.pending.borrow().len() as u64, "directory"))
            } else {
                String::new()
            },
        );
        result
    }
}

/// How long a roll-up drain runs before it checks whether a message is waiting.
/// Matches the implicit batch's cap (`batch.rs`): a live change waits about this
/// long, at most, behind a drain.
const DRAIN_SLICE: Duration = Duration::from_millis(250);

/// What one [`PendingRollups::drain`] did.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Drained {
    /// A `dir_stats` row changed, so the panes owe a refresh.
    pub(super) moved: bool,
    /// The drain gave the writer back with roll-ups still owed.
    pub(super) owes_more: bool,
}

/// The parent and depth of every directory on the drained chains, read once per
/// directory however many chains share it.
#[derive(Default)]
struct Chains {
    /// `id` → (parent id, or 0 above the root; depth, the root at 0).
    known: HashMap<i64, (i64, usize)>,
}

impl Chains {
    fn parent_of(&self, id: i64) -> Option<i64> {
        self.known.get(&id).map(|&(parent, _)| parent).filter(|&p| p != 0)
    }

    /// `id`'s depth, walking up until a directory already known (or the top).
    /// `None` when the chain can't be read: a gone entry (nothing owed; its
    /// delete paid the chain), a failed read (handed to `repairs`, like every
    /// repair read), or a loop (`ChainSteps` marks the index for a rebuild).
    fn depth_of(&mut self, conn: &rusqlite::Connection, id: i64, repairs: &DeferredRepairs) -> Option<usize> {
        let mut climbed = Vec::new();
        let mut steps = ChainSteps::new("ancestor roll-up", id);
        let mut current = id;
        let base = loop {
            if let Some(&(_, depth)) = self.known.get(&current) {
                break depth;
            }
            if !steps.another(conn) {
                return None;
            }
            let parent = if current == ROOT_ID {
                0
            } else {
                match IndexStore::get_parent_id(conn, current) {
                    Ok(Some(parent)) => parent,
                    Ok(None) => return None,
                    Err(e) => {
                        log::debug!(target: "indexing::writer", "ancestor roll-up: parent lookup failed for id={current}: {e}");
                        repairs.queue(current, "ancestor roll-up parent lookup");
                        return None;
                    }
                }
            };
            climbed.push((current, parent));
            if parent == 0 {
                // The top: `current` sits at depth 0 and everything climbed below it.
                let (top, _) = climbed.pop().expect("just pushed");
                self.known.insert(top, (0, 0));
                break 0;
            }
            current = parent;
        };
        // Number what we climbed from the known directory back down to `id`.
        let mut depth = base;
        for &(dir, parent) in climbed.iter().rev() {
            depth += 1;
            self.known.insert(dir, (parent, depth));
        }
        self.known.get(&id).map(|&(_, depth)| depth)
    }
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
/// rows roll back, so everything the drain was asked for is queued again.
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
    wants_the_writer: &dyn Fn() -> bool,
) -> Drained {
    if !conn.is_autocommit() {
        return Drained::default();
    }
    let mut drained = Drained::default();
    if !rollups.is_empty() {
        let asked: Vec<i64> = rollups.pending.borrow().iter().copied().collect();
        let in_transaction = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
        drained = rollups.drain(conn, repairs, DRAIN_SLICE, wants_the_writer);
        if in_transaction {
            // A fatal error mid-drain can roll the transaction back on its own; a
            // failed COMMIT leaves it open. Either way none of it landed.
            let committed = !conn.is_autocommit() && conn.execute_batch("COMMIT").is_ok();
            if !committed {
                if !conn.is_autocommit() {
                    let _ = conn.execute_batch("ROLLBACK");
                }
                log::debug!(
                    target: "indexing::writer",
                    "Ancestor roll-up: the drain's transaction didn't commit; {} queued again",
                    pluralize(asked.len() as u64, "directory"),
                );
                for id in asked {
                    rollups.queue(id);
                }
                return Drained {
                    moved: false,
                    owes_more: true,
                };
            }
        }
    }
    if !drained.owes_more && !repairs.is_empty() {
        repairs.drain(conn);
    }
    drained
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
mod tests {
    use super::*;

    /// The whole point of the set: a burst of roots sharing one parent owes ONE
    /// walk, not one per root.
    #[test]
    fn a_burst_sharing_one_parent_queues_one_id() {
        let rollups = PendingRollups::new(Arc::new(AtomicU64::new(0)));
        for _ in 0..10_000 {
            rollups.queue(42);
        }
        assert_eq!(rollups.pending.borrow().len(), 1, "one parent, one queued roll-up");
    }

    /// Distinct parents each keep their own entry: coalescing must never lose a
    /// chain that nothing else is going to repair.
    #[test]
    fn distinct_parents_all_survive() {
        let rollups = PendingRollups::new(Arc::new(AtomicU64::new(0)));
        for id in 1..=50 {
            rollups.queue(id);
        }
        assert_eq!(rollups.pending.borrow().len(), 50);
    }

    /// A drain with a message waiting gives the writer back after its slice, keeps
    /// what it still owes, and the drains that pick it up later land the same
    /// answer one uninterrupted drain would. GitHub issue #1480: live changes must
    /// never wait out a whole drain.
    #[test]
    fn a_waiting_message_takes_the_writer_back_and_nothing_owed_is_lost() {
        use crate::indexing::stress_test_helpers::check_db_consistency;
        use crate::indexing::writer::entry_fixtures::{dir_entry, file_entry};
        use crate::indexing::writer::tests::{settle_the_writer, setup_db};
        use crate::indexing::writer::{AggSource, IndexWriter, WriteMessage};

        const WIDTH: i64 = 20;
        let (db_path, _dir) = setup_db();
        let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
        let mut entries = vec![dir_entry(10, ROOT_ID, "big")];
        let mids: Vec<i64> = (0..WIDTH).map(|index| 100 + index * 2).collect();
        for &mid in &mids {
            entries.push(dir_entry(mid, 10, &format!("mid-{mid}")));
            entries.push(file_entry(mid + 1, mid, "leaf", 1000));
        }
        writer.send(WriteMessage::InsertEntriesV2(entries)).unwrap();
        writer
            .send(WriteMessage::ComputeAllAggregates {
                source: AggSource::Maps,
            })
            .unwrap();
        settle_the_writer(&writer);
        writer.shutdown();

        // Every mid, `big`, and the root drift low, so each needs a recompute.
        let conn = IndexStore::open_write_connection(&db_path).unwrap();
        conn.execute("UPDATE dir_stats SET recursive_logical_size = 0", [])
            .unwrap();
        let rollups = PendingRollups::new(Arc::new(AtomicU64::new(0)));
        for &mid in &mids {
            rollups.queue(mid);
        }
        let repairs = DeferredRepairs::new();

        // A zero slice and a message always waiting: one recompute per drain.
        let first = rollups.drain(&conn, &repairs, Duration::ZERO, &|| true);
        assert!(first.owes_more, "a waiting message takes the writer back");
        assert_eq!(rollups.recomputes.load(Ordering::Relaxed), 1);
        assert_eq!(
            rollups.pending.borrow().len(),
            WIDTH as usize,
            "what's owed stays queued: the other mids, plus `big`, which the first rewrite owes"
        );

        let mut drains = 1;
        while !rollups.is_empty() {
            rollups.drain(&conn, &repairs, Duration::ZERO, &|| true);
            drains += 1;
            assert!(drains <= WIDTH + 2, "every drain makes progress");
        }
        assert_eq!(
            rollups.recomputes.load(Ordering::Relaxed),
            WIDTH as u64 + 2,
            "sliced, each directory is still recomputed once: the mids, `big`, the root"
        );
        let big = IndexStore::get_dir_stats_by_id(&conn, 10).unwrap().unwrap();
        assert_eq!(big.recursive_logical_size, WIDTH as u64 * 1000);
        let root = IndexStore::get_dir_stats_by_id(&conn, ROOT_ID).unwrap().unwrap();
        assert_eq!(root.recursive_logical_size, WIDTH as u64 * 1000);
        check_db_consistency(&conn);
    }
}
