//! The per-thread read-connection cache, and the one way to close what the
//! caches hold on a database that is going away.
//!
//! Its own file because it is its own mechanism with its own state (the live
//! count, the registry of open slots, the retirement epoch). Everything here is
//! re-exported from `sqlite_util`, which is the path callers use.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};

use rusqlite::Connection;

use super::{READ_CONNECTION_BUDGET, READ_PAGE_CACHE_KIB, SHARED_PAGE_CACHE_BYTES};
use crate::ignore_poison::IgnorePoison;

/// A small per-thread LRU of open read connections, keyed by db path plus the
/// caller's invalidation generation.
///
/// Both read paths (`indexing::read::enrichment`'s `ReadPool` and
/// `ImportanceIndex`) keep their connections in a thread-local so enrichment
/// never waits on another thread on the hot path. Holding ONE slot made that
/// expensive in the ordinary two-pane case: a thread alternating between the
/// left pane's volume and the right pane's closed and reopened on every
/// alternation, re-running the pragmas and the collation registration and
/// throwing away the connection's whole `prepare_cached` statement cache —
/// recompiling those statements is the expensive part. A handful of slots is
/// affordable because [`READ_PAGE_CACHE_KIB`] is sized against
/// [`READ_CONNECTION_BUDGET`] rather than against one connection, so a slot costs
/// 128 KiB of the page ceiling rather than 8 MiB of it.
///
/// Every entry is counted in [`live_read_connections`], including the ones this
/// cache evicts and the ones it takes down with a dying thread, so the budget is
/// observable rather than hoped for.
///
/// The cache itself lives in a `thread_local!` `RefCell` with no lock. Each
/// connection sits in its own [`Slot`], a mutex only its thread takes on the hot
/// path (uncontended, so a few nanoseconds), and the one other taker is
/// [`retire_read_connections`], which closes the connection from the retiring
/// thread. ❌ Don't put the whole cache behind a mutex: the slot is what makes the
/// close reachable, and a cache-wide lock would serialize every reader.
pub struct ThreadConnCache {
    /// Most-recently-used first. Never longer than `capacity`. An entry whose slot
    /// a retirement emptied stays until this thread's next use sweeps it.
    entries: Vec<CachedConn>,
    capacity: usize,
    /// The [`RETIRE_EPOCH`] this cache last swept its emptied slots against.
    swept_at: u64,
}

/// One cached connection's home: `None` once it was closed, by its own cache or
/// by a retirement from any thread. Whoever takes the connection out counts the
/// close, so it's counted once.
type Slot = Arc<Mutex<Option<Connection>>>;

/// One cached read connection.
struct CachedConn {
    db_path: PathBuf,
    /// The caller's invalidation generation it was opened under.
    generation: u64,
    slot: Slot,
}

impl CachedConn {
    fn is_open(&self) -> bool {
        self.slot.lock_ignore_poison().is_some()
    }
}

/// Slots per thread. Two covers the ordinary two-pane case (left pane on the
/// boot disk, right pane on a NAS share); the third absorbs a background reader
/// (search, the importance scheduler) landing on the same blocking thread
/// without evicting either pane.
pub const THREAD_CONN_SLOTS: usize = 3;

/// Read connections the process's [`ThreadConnCache`]s hold right now.
static LIVE_READ_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

/// Latched the first time the count passes [`READ_CONNECTION_BUDGET`], so the
/// warning is one line rather than one per open from then on.
static READ_BUDGET_EXCEEDED: AtomicBool = AtomicBool::new(false);

/// How many read connections the process's [`ThreadConnCache`]s hold right now,
/// across every thread.
///
/// That is the DURABLE read population and the term the sizing is about: those
/// connections live as long as their thread, so their count tracks tokio's
/// blocking pool. Multiply by [`READ_PAGE_CACHE_KIB`] for their share of SQLite's
/// global ceiling on retained pages, and read it against
/// [`READ_CONNECTION_BUDGET`], which is what the page cache is sized for.
///
/// ⚠️ NOT every read connection in the process. The media, agent, and
/// operation-log stores open a read connection per call and drop it, so they
/// never enter this count; they add [`READ_PAGE_CACHE_KIB`] apiece to
/// `pGroup->nMaxPage` only for the life of the call.
pub fn live_read_connections() -> usize {
    LIVE_READ_CONNECTIONS.load(Ordering::Relaxed)
}

/// Count one newly opened read connection, and say so once if that puts the
/// process past the budget its page cache was sized for.
fn count_read_connection_opened() {
    let live = LIVE_READ_CONNECTIONS.fetch_add(1, Ordering::Relaxed) + 1;
    if live > READ_CONNECTION_BUDGET && !READ_BUDGET_EXCEEDED.swap(true, Ordering::Relaxed) {
        let ceiling_mib = (live * READ_PAGE_CACHE_KIB as usize) / 1024;
        log::warn!(
            target: "sqlite",
            "{} open, past the {READ_CONNECTION_BUDGET} the page cache is sized for; their share of SQLite's global page ceiling is now ~{ceiling_mib} MiB against a {} MiB slab, so the slab can run permanently full",
            crate::pluralize::pluralize(live as u64, "read connection"),
            SHARED_PAGE_CACHE_BYTES / (1024 * 1024)
        );
    }
}

/// Close the connection in `slot` if it's still there, and count it. The
/// connection drops AFTER the slot's lock is released: closing one can
/// checkpoint, and nobody else should wait on that.
fn close(slot: &Slot) {
    let conn = slot.lock_ignore_poison().take();
    if conn.is_some() {
        LIVE_READ_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
    }
    drop(conn);
}

impl ThreadConnCache {
    /// An empty cache holding at most `capacity` connections.
    pub const fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity,
            swept_at: 0,
        }
    }

    /// Run `f` against the connection for `(db_path, generation)`, opening one
    /// through `open` when this thread holds no live match.
    ///
    /// A hit moves the entry to the front; a miss evicts the least recently used
    /// entry once the cache is full. An entry for `db_path` at a DIFFERENT
    /// generation is closed before the reopen, so a caller's `invalidate()`
    /// still retires the stale connection rather than leaving two around.
    ///
    /// `f` runs under the connection's slot lock, so a retirement of `db_path`
    /// waits for it to finish. ❌ Never retire or delete a database from inside
    /// `f`: the retirement would wait on the lock its own thread holds.
    pub fn with<T, E>(
        &mut self,
        db_path: &Path,
        generation: u64,
        open: impl FnOnce(&Path) -> Result<Connection, E>,
        f: impl FnOnce(&Connection) -> T,
    ) -> Result<T, E> {
        let epoch = RETIRE_EPOCH.load(Ordering::Acquire);
        if epoch != self.swept_at {
            self.entries.retain(CachedConn::is_open);
            self.swept_at = epoch;
        }
        if let Some(hit) = self
            .entries
            .iter()
            .position(|entry| entry.db_path == db_path && entry.generation == generation)
        {
            let entry = self.entries.remove(hit);
            let guard = entry.slot.lock_ignore_poison();
            if let Some(conn) = guard.as_ref() {
                let answer = f(conn);
                drop(guard);
                self.entries.insert(0, entry);
                return Ok(answer);
            }
            // A retirement emptied it after the sweep above: open afresh below.
        }

        // Close a same-path entry at a stale generation: the caller invalidated
        // it, so it must not linger behind the new one.
        self.entries.retain(|entry| {
            let stale = entry.db_path == db_path;
            if stale {
                close(&entry.slot);
            }
            !stale
        });
        let slot: Slot = Arc::new(Mutex::new(Some(open(db_path)?)));
        count_read_connection_opened();
        if self.entries.len() >= self.capacity
            && let Some(evicted) = self.entries.pop()
        {
            close(&evicted.slot);
        }
        // Locked BEFORE it's registered, so a retirement that finds it waits for
        // `f` rather than closing it under this first use.
        let guard = slot.lock_ignore_poison();
        register(db_path, &slot);
        self.entries.insert(
            0,
            CachedConn {
                db_path: db_path.to_path_buf(),
                generation,
                slot: Arc::clone(&slot),
            },
        );
        let conn = guard
            .as_ref()
            .expect("a slot holds its connection until someone takes it, and this thread holds its lock");
        Ok(f(conn))
    }

    /// Test-only: the generation this thread holds an open connection at for
    /// `db_path`, or `None` when it holds no open connection to it.
    #[cfg(any(test, feature = "testing"))]
    pub fn generation_for(&self, db_path: &Path) -> Option<u64> {
        self.entries
            .iter()
            .find(|entry| entry.db_path == db_path && entry.is_open())
            .map(|entry| entry.generation)
    }

    /// Test-only: how many open connections this thread currently holds.
    #[cfg(any(test, feature = "testing"))]
    pub fn len(&self) -> usize {
        self.entries.iter().filter(|entry| entry.is_open()).count()
    }

    /// Test-only: whether this thread holds no open connections at all. Paired
    /// with [`len`](Self::len) because clippy won't take one without the other.
    #[cfg(any(test, feature = "testing"))]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Drop for ThreadConnCache {
    /// A thread dying takes its connections with it, so the process-wide count
    /// has to follow. Tokio retires an idle blocking thread after ten seconds,
    /// so this runs routinely rather than only at shutdown.
    fn drop(&mut self) {
        for entry in &self.entries {
            close(&entry.slot);
        }
    }
}

// ── Closing cached read connections ──────────────────────────────────

/// Bumped by every retirement, so a [`ThreadConnCache`] sweeps the slots a
/// retirement emptied at its next use, and a thread with nothing retired pays one
/// atomic load per use.
static RETIRE_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Every open slot in the process, by db path, so a retirement on any thread can
/// reach them. Weak, so a slot its cache dropped goes with it; dead entries are
/// pruned on each registration.
type Registry = Vec<(PathBuf, Weak<Mutex<Option<Connection>>>)>;
static OPEN_SLOTS: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn register(db_path: &Path, slot: &Slot) {
    let mut open = OPEN_SLOTS.lock_ignore_poison();
    open.retain(|(_, slot)| slot.strong_count() > 0);
    open.push((db_path.to_path_buf(), Arc::downgrade(slot)));
}

/// Close every cached read connection whose db path `matches`, on whichever
/// thread's cache holds it, before returning.
fn retire_where(matches: impl Fn(&Path) -> bool) {
    let slots: Vec<Slot> = OPEN_SLOTS
        .lock_ignore_poison()
        .iter()
        .filter(|(path, _)| matches(path))
        .filter_map(|(_, slot)| slot.upgrade())
        .collect();
    // Outside the registry lock: a slot's lock can be held for a whole query.
    for slot in &slots {
        close(slot);
    }
    RETIRE_EPOCH.fetch_add(1, Ordering::AcqRel);
}

/// Close every read connection the process's caches hold to `db_path`, on every
/// thread, before returning.
///
/// For a database that is going away (deleted, or its volume stopped). A delete
/// needs it: Windows refuses to delete a file any handle holds (#1226), and POSIX
/// keeps an unlinked file's blocks allocated until its last handle closes.
///
/// - A connection in use on its thread closes once that read finishes: the call
///   waits for it.
/// - Connections opened AFTER the call are untouched, so a database recreated
///   under the same path is cached like any other. A delete calls it on both
///   sides of the unlink ([`delete_database`](super::delete_database) does).
/// - ⚠️ Only the CACHED read connections: a writer, or a connection a store opens
///   per call, is its owner's to stop first.
pub fn retire_read_connections(db_path: &Path) {
    retire_where(|path| path == db_path);
}

/// [`retire_read_connections`] for every database under `dir`, at any depth: for
/// a whole folder that is going away ("Delete all Cmdr data" moves the data dir,
/// which Windows refuses while a file in it is open).
pub fn retire_read_connections_under(dir: &Path) {
    retire_where(|path| path.starts_with(dir));
}
