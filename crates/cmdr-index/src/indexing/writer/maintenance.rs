//! Periodic DB housekeeping run on the writer thread: the WAL checkpoint, which
//! truncates the WAL file once readers permit, and the busy handler's one-line
//! contention episodes. (The freelist vacuum lives in `vacuum.rs`.) The checkpoint
//! is fired by a background timer and also right after a full scan; it mutates no
//! `entries` rows, so it doesn't bump the writer generation. Its errors go through
//! `IndexFailureSignal::note_maintenance`, never `note`: a failed housekeeping pass
//! is retried next tick, not a reason to stop the index.

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::indexing::IndexFailureSignal;
use crate::indexing::store::IndexStoreError;
use cmdr_fs::pluralize::pluralize;

// ── Busy-handler checkpoint suppression ──────────────────────────────

thread_local! {
    /// Set while THIS writer thread is inside [`handle_wal_checkpoint`]'s
    /// `PRAGMA wal_checkpoint(TRUNCATE)`, which deliberately waits readers out
    /// (to ~attempt 51, ~250 ms) before degrading to PASSIVE. The busy handler
    /// reads it to keep that expected wait at debug instead of escalating to warn.
    static IN_WAL_CHECKPOINT: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard marking the writer thread as inside the WAL checkpoint's reader
/// wait. Resets the flag on drop, so a panic mid-checkpoint can't leave it stuck.
struct WalCheckpointGuard;

impl WalCheckpointGuard {
    fn enter() -> Self {
        IN_WAL_CHECKPOINT.with(|f| f.set(true));
        WalCheckpointGuard
    }
}

impl Drop for WalCheckpointGuard {
    fn drop(&mut self) {
        IN_WAL_CHECKPOINT.with(|f| f.set(false));
    }
}

// ── Busy episodes (one log line per lock wait) ───────────────────────

/// Last attempt the busy handler retries; past it, it returns `false` and SQLite
/// gives the write up with `SQLITE_BUSY`. At 5 ms per retry that caps a wait at
/// ~255 ms — short on purpose, since the writer thread stalls every queued write
/// behind it.
pub(super) const BUSY_GIVE_UP_ATTEMPT: i32 = 50;

/// One contention episode: SQLite couldn't take the write lock, so it drove the
/// busy handler in a retry loop until the lock came free (or the handler gave up).
#[derive(Clone, Copy)]
struct BusyEpisode {
    /// When the handler first fired for this locking event.
    started: Instant,
    /// Highest `attempt` the handler saw. Invocations = `peak_attempt + 1`.
    peak_attempt: i32,
    /// Whether any part of the wait happened inside the WAL checkpoint's
    /// deliberate reader wait. Captured here because the flag is already reset
    /// by the time the episode is flushed.
    in_checkpoint: bool,
}

thread_local! {
    /// The episode in progress on THIS writer thread, if any. Per-thread because
    /// each writer owns its own connection and drives its own busy handler.
    static BUSY_EPISODE: Cell<Option<BusyEpisode>> = const { Cell::new(None) };
}

/// Whether the writer's busy handler escalates to warn at `attempt`. Sustained
/// contention (>= 20 attempts, ~100 ms lock wait) is a genuine stall signal —
/// except during the WAL checkpoint's deliberate reader wait, which is working
/// as designed and stays at debug. Pure, so the policy is unit-testable.
pub(super) fn busy_handler_escalates(attempt: i32, in_checkpoint: bool) -> bool {
    attempt >= 20 && !in_checkpoint
}

/// The one line an episode emits: `(warn, message)`. Pure, so the shape and the
/// warn-vs-debug policy are unit-testable without touching the logger.
fn busy_episode_summary(peak_attempt: i32, waited: Duration, in_checkpoint: bool) -> (bool, String) {
    let attempts = pluralize((peak_attempt as u64) + 1, "attempt");
    let ms = waited.as_millis();
    let mut message = if peak_attempt > BUSY_GIVE_UP_ATTEMPT {
        format!("writer gave up waiting for the write lock after {ms} ms over {attempts}")
    } else {
        format!("writer waited {ms} ms over {attempts} for the write lock")
    };
    if in_checkpoint {
        message.push_str(" (WAL checkpoint reader wait)");
    }
    (busy_handler_escalates(peak_attempt, in_checkpoint), message)
}

/// Record one busy-handler invocation. Called from the handler itself, which
/// SQLite drives with `attempt` counting from 0 for each new locking event — so
/// a 0 both opens a fresh episode and proves the previous one is over (the write
/// either got its lock or gave up), which is when the previous one is logged.
pub(super) fn note_busy_attempt(attempt: i32) {
    if attempt == 0 {
        flush_busy_episode();
    }
    BUSY_EPISODE.with(|slot| {
        let in_checkpoint = IN_WAL_CHECKPOINT.with(|f| f.get());
        let episode = match slot.get() {
            Some(mut e) => {
                e.peak_attempt = e.peak_attempt.max(attempt);
                e.in_checkpoint |= in_checkpoint;
                e
            }
            None => BusyEpisode {
                started: Instant::now(),
                peak_attempt: attempt,
                in_checkpoint,
            },
        };
        slot.set(Some(episode));
    });
}

/// Close the open episode, if any, and log its one summary line. Called by the
/// writer loop after every message: the busy handler has no "you got the lock"
/// callback, so the episode is closed from the outside once the work that
/// contended is done. (A back-to-back episode within the same message is closed
/// by [`note_busy_attempt`]'s `attempt == 0` instead.)
pub(super) fn flush_busy_episode() {
    let Some(episode) = BUSY_EPISODE.with(|slot| slot.take()) else {
        return;
    };
    let (warn, message) = busy_episode_summary(episode.peak_attempt, episode.started.elapsed(), episode.in_checkpoint);
    if warn {
        log::warn!(target: "stall_probe::sqlite_busy", "{message}");
    } else {
        log::debug!(target: "stall_probe::sqlite_busy", "{message}");
    }
}

/// Periodically TRUNCATE the WAL file so its high-water mark doesn't sit on
/// disk indefinitely. SQLite's `wal_autocheckpoint` runs in PASSIVE mode and
/// only moves pages from WAL to the main file; it never shrinks the file
/// itself. After a big scan the WAL can balloon to 1+ GB, and without an
/// explicit TRUNCATE that file size persists until the next app restart.
///
/// TRUNCATE blocks waiting for readers, invoking this connection's busy handler
/// (installed in `writer/mod.rs::spawn`) while it waits. That handler — NOT the
/// `busy_timeout = 5000` pragma, which installing a `busy_handler` overrides —
/// caps the wait at ~250 ms (it sleeps 5 ms per retry and gives up at attempt 51),
/// after which the call degrades to PASSIVE semantics (busy code = 1 in the return
/// tuple): pages still get checkpointed, the file just doesn't shrink this time.
/// Next tick tries again. No error path needed. The short cap is deliberate: this
/// runs on the writer thread, so a multi-second block would stall every live write
/// queued behind it.
///
/// A `WalCheckpointGuard` brackets the TRUNCATE, so the busy handler stays at
/// debug for the whole reader wait rather than escalating to warn past attempt 20
/// — a persistent reader here is working-as-designed, not a stall. Every OTHER
/// writer contention still warns past attempt 20.
/// Run the periodic WAL checkpoint, or park it until the open batch transaction
/// commits. Returns whether the checkpoint was parked.
///
/// `PRAGMA wal_checkpoint(TRUNCATE)` cannot run inside a transaction: SQLite
/// refuses it with `SQLITE_LOCKED` ("database table is locked"). A bulk run (a
/// journal replay, a network walk) holds an explicit transaction most of the time,
/// so an undeferred maintenance tick would fail on nearly every tick, exactly when
/// write volume is highest. Parking it means the truncate happens right after the
/// next commit, which is when it can actually reclaim the space. That only bounds
/// the WAL because those runs commit in chunks (`watch/DETAILS.md` § "A replay
/// commits in chunks").
pub(super) fn request_wal_checkpoint(conn: &rusqlite::Connection, signal: &IndexFailureSignal, deferred: &mut bool) {
    if conn.is_autocommit() {
        handle_wal_checkpoint(conn, signal);
    } else {
        log::debug!("Writer: wal_checkpoint deferred until the open transaction commits");
        *deferred = true;
    }
}

/// Run a checkpoint parked by [`request_wal_checkpoint`], if any. Called after a
/// commit. Re-checks `is_autocommit`: a COMMIT that itself failed leaves the
/// transaction open, and the checkpoint stays parked for the next chance rather
/// than reporting the same non-error again.
pub(super) fn run_deferred_wal_checkpoint(
    conn: &rusqlite::Connection,
    signal: &IndexFailureSignal,
    deferred: &mut bool,
) {
    if *deferred && conn.is_autocommit() {
        *deferred = false;
        handle_wal_checkpoint(conn, signal);
    }
}

fn handle_wal_checkpoint(conn: &rusqlite::Connection, signal: &IndexFailureSignal) {
    let _guard = WalCheckpointGuard::enter();
    // `PRAGMA wal_checkpoint(TRUNCATE)` returns a single row with three
    // columns: (busy, log_size, checkpointed). `busy = 0` means everything
    // got checkpointed AND the file was truncated; `busy = 1` means at least
    // one reader was still on the WAL so the file couldn't shrink (pages
    // were still copied to the main file). Either is a success from the
    // caller's POV — only a SQL error means something is actually wrong.
    let result: rusqlite::Result<(i64, i64, i64)> = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    });
    match result {
        // Nothing in the WAL, nothing copied: the checkpoint ran and had no work.
        // Silent, because "done (0 of 0 pages)" was ~160 lines an hour saying that
        // the last one already did the job. A checkpoint that MOVED something is
        // still on the line, and so is every busy or failing one.
        Ok((0, 0, 0)) => {}
        Ok((0, log_size, checkpointed)) => {
            log::debug!(
                "Writer: wal_checkpoint TRUNCATE done ({checkpointed} of {})",
                pluralize(log_size as u64, "page")
            );
        }
        Ok((_, log_size, checkpointed)) => {
            // Busy: readers blocking the truncate. Pages still got written
            // to the main file; the WAL file just didn't shrink this tick.
            log::debug!(
                "Writer: wal_checkpoint partial ({checkpointed} of {}, blocked by readers)",
                pluralize(log_size as u64, "page")
            );
        }
        // Maintenance: an error here logs and the next tick retries; only proven
        // corruption stops the index (cmdr-reports#105).
        Err(e) => {
            signal.note_maintenance(&IndexStoreError::from(e), "wal_checkpoint");
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::store::{EntryRow, ROOT_ID};
    use crate::indexing::writer::tests::setup_db;
    use crate::indexing::writer::{IndexWriter, WriteMessage};

    // ── Busy-episode summary ─────────────────────────────────────────

    /// One line per episode carries what the per-attempt ladder used to spread
    /// over dozens of lines: how long the writer waited and how many retries it
    /// took. Short contention is routine, so it stays at debug.
    #[test]
    fn busy_episode_summary_reports_total_wait_and_attempts() {
        let (warn, msg) = busy_episode_summary(4, Duration::from_millis(25), false);
        assert!(!warn, "a five-attempt wait is routine contention");
        assert_eq!(msg, "writer waited 25 ms over 5 attempts for the write lock");
    }

    /// Sustained contention (the old attempt >= 20 warn threshold) still warns,
    /// now once per episode instead of once per attempt.
    #[test]
    fn busy_episode_summary_warns_on_sustained_contention() {
        let (warn, msg) = busy_episode_summary(26, Duration::from_millis(340), false);
        assert!(warn, "27 attempts is a genuine stall signal");
        assert_eq!(msg, "writer waited 340 ms over 27 attempts for the write lock");
    }

    /// The WAL checkpoint's TRUNCATE deliberately waits readers out to ~attempt
    /// 51, so its episode stays at debug and says so.
    #[test]
    fn busy_episode_summary_stays_quiet_during_the_checkpoint_reader_wait() {
        let (warn, msg) = busy_episode_summary(50, Duration::from_millis(255), true);
        assert!(!warn, "the checkpoint's reader wait is working as designed");
        assert!(
            msg.ends_with("(WAL checkpoint reader wait)"),
            "the checkpoint context belongs in the line, got: {msg}"
        );
    }

    /// Past the retry cap the handler returns false and SQLite gives up, which
    /// the summary must say outright — that's the difference between "we waited"
    /// and "the write didn't get the lock".
    #[test]
    fn busy_episode_summary_says_it_gave_up_past_the_retry_cap() {
        let (warn, msg) = busy_episode_summary(BUSY_GIVE_UP_ATTEMPT + 1, Duration::from_millis(260), false);
        assert!(warn);
        assert_eq!(
            msg,
            "writer gave up waiting for the write lock after 260 ms over 52 attempts"
        );
    }

    /// A maintenance tick that lands mid-batch (the replay wraps its whole run in
    /// one `BeginTransaction`) must not call the TRUNCATE at all: SQLite refuses a
    /// checkpoint inside an open transaction with `SQLITE_LOCKED`, so every tick
    /// would report a storage error that isn't one. The checkpoint is deferred to
    /// the commit instead, so the WAL still gets truncated — which is the point of
    /// the tick.
    #[test]
    fn wal_checkpoint_defers_out_of_an_open_transaction() {
        let (db_path, _dir) = setup_db();
        let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();

        let transaction = writer.begin_transaction().expect("begin");
        let entries: Vec<EntryRow> = (0..2000)
            .map(|i| EntryRow {
                id: 300 + i,
                parent_id: ROOT_ID,
                name: format!("deferred-checkpoint-entry-{i:08}"),
                is_directory: false,
                is_symlink: false,
                logical_size: Some(1024),
                physical_size: Some(1024),
                modified_at: None,
                inode: None,
            })
            .collect();
        writer.send(WriteMessage::InsertEntriesV2(entries)).unwrap();

        // The maintenance tick, arriving while the batch transaction is open.
        writer.send(WriteMessage::WalCheckpoint).unwrap();
        writer.flush_blocking().unwrap();

        assert_eq!(
            writer.failure_signal().note_count(),
            0,
            "a checkpoint skipped because a transaction is open is not a storage error"
        );

        let wal_path = format!("{}-wal", db_path.display());
        let wal_size_before = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert!(
            wal_size_before > 0,
            "test setup: expected a grown WAL, got {wal_size_before}"
        ); // allowed-pluralize-noun: assertion-failure-only message guarded by `> 0`

        transaction.commit().expect("commit");
        writer.flush_blocking().unwrap();

        let wal_size_after = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert!(
            wal_size_after < wal_size_before,
            "the deferred checkpoint must run once the transaction commits; \
             before={wal_size_before}, after={wal_size_after}"
        );

        writer.shutdown();
    }

    /// End-to-end check: after inserts have grown the WAL, `WalCheckpoint`
    /// shrinks the on-disk WAL file. The WAL file is `db_path` + "-wal";
    /// after a successful TRUNCATE checkpoint with no readers, it should
    /// drop to zero bytes (or a small header).
    #[test]
    fn handle_wal_checkpoint_truncates_wal_file() {
        let (db_path, _dir) = setup_db();
        let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();

        // Grow the WAL with a non-trivial insert batch.
        let entries: Vec<EntryRow> = (0..2000)
            .map(|i| EntryRow {
                id: 200 + i,
                parent_id: ROOT_ID,
                name: format!("wal-test-entry-{i:08}"),
                is_directory: false,
                is_symlink: false,
                logical_size: Some(1024),
                physical_size: Some(1024),
                modified_at: None,
                inode: None,
            })
            .collect();
        writer.send(WriteMessage::InsertEntriesV2(entries)).unwrap();
        writer.flush_blocking().unwrap();

        let wal_path = format!("{}-wal", db_path.display());
        let wal_size_before = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert!(
            wal_size_before > 0,
            "expected WAL file to have grown after 2000 inserts; got {} bytes",
            wal_size_before // allowed-pluralize-noun: assertion-failure-only message; the assertion is `> 0`, so when it fires `wal_size_before == 0` and "0 bytes" reads correctly
        );

        writer.send(WriteMessage::WalCheckpoint).unwrap();
        writer.flush_blocking().unwrap();

        let wal_size_after = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert!(
            wal_size_after < wal_size_before,
            "WalCheckpoint should shrink the WAL file; before={wal_size_before}, after={wal_size_after}"
        );

        writer.shutdown();
    }
}
