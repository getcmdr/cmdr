//! Freelist reclamation on the writer thread: the 30 s `IncrementalVacuum` tick
//! sets a quota, and [`VacuumWork`] pays it in bounded slices so no single
//! vacuum holds everything queued behind it. A rebuild after `TruncateData`
//! pauses it until the walk's full aggregate. Errors go through
//! `IndexFailureSignal::note_maintenance`: the next tick retries.

use std::time::{Duration, Instant};

use crate::indexing::IndexFailureSignal;
use crate::indexing::store::IndexStoreError;
use cmdr_fs::pluralize::pluralize;

/// Cap thresholds for the tiered incremental-vacuum policy. Below `MIN`,
/// holding the write lock isn't worth the work. Between `MIN` and `BACKLOG`,
/// keep the original steady-state cap so concurrent operations barely notice.
/// Above `BACKLOG`, ramp the cap to drain backlogs (post-truncate, post-replay,
/// or DBs migrated from older versions that accumulated free pages) in tens of
/// minutes instead of hours.
const VACUUM_MIN_FREELIST: i64 = 1_000;
const VACUUM_STEADY_CAP: i64 = 2_000;
const VACUUM_BACKLOG_THRESHOLD: i64 = 20_000;
const VACUUM_BACKLOG_CAP: i64 = 20_000;

/// Pick the per-tick `incremental_vacuum` page quota given the current
/// `freelist_count`. Pure so it can be tested in isolation.
///
/// Tiered: skip the no-op lock acquisition when the freelist is small, and take
/// a bigger quota when there's real backlog so it drains in tens of minutes. The
/// quota is what a tick reclaims in TOTAL; [`VacuumWork`] pays it in slices.
fn pick_vacuum_cap(freelist: i64) -> Option<i64> {
    if freelist < VACUUM_MIN_FREELIST {
        None
    } else if freelist < VACUUM_BACKLOG_THRESHOLD {
        Some(VACUUM_STEADY_CAP)
    } else {
        Some(VACUUM_BACKLOG_CAP)
    }
}

/// Most pages one vacuum slice frees, and the wall time it may take. Whichever
/// comes first ends the slice.
///
/// Measured (2026-11-25, `sqlite3` 3.54 on an APFS clone of a 6.99M-row root
/// index, 266K pages, right after the rebuild's `DELETE`s, M-series SSD): 1,000
/// pages took 0.13–0.23 s, 2,000 took 0.19–0.24 s, 5,000 took 0.94 s, 20,000
/// took 3.1 s. In the app, under a live rebuild, the same 20,000 took 10.3 s
/// (0.52 ms a page), and later 20,000-page ticks took about 2 s each (dev log,
/// same day). The cost per page moves 5× with load, so time is what bounds the
/// slice: 250 ms matches the busy handler's ~255 ms lock-wait cap, the longest
/// the writer otherwise lets one thing hold everything queued behind it. The
/// page cap keeps one slice's WAL growth near 8 MB on a fast, idle machine,
/// where 2,000 pages land right around the time budget.
const VACUUM_SLICE_PAGES: i64 = 2_000;
const VACUUM_SLICE_BUDGET: Duration = Duration::from_millis(250);

/// The writer's freelist housekeeping: what the 30 s tick still owes, paid in
/// bounded slices, and whether a rebuild has asked it to wait.
///
/// A tick (`IncrementalVacuum`) sets a quota and runs ONE slice; the writer runs
/// the rest between messages, whenever its channel is empty (`owes_more` and
/// `run_slice`), so a message never waits behind more than one slice. Under a
/// queue that never empties, each tick still pays one slice. ❌ The writer can't
/// requeue a message to itself: it would hold a sender, so its channel would
/// never disconnect, and a self-send into a full bounded channel would deadlock.
pub(super) struct VacuumWork {
    /// Pages the latest tick still wants reclaimed.
    owed: i64,
    /// Set by `TruncateData`, cleared by the next full aggregate: the rebuild
    /// refills the freelist before it grows the file, so a vacuum meanwhile only
    /// moves pages the walk then writes the file back out over.
    paused_for_rebuild: bool,
    slice_pages: i64,
    slice_budget: Duration,
}

impl VacuumWork {
    pub(super) fn new() -> Self {
        Self::with_slice(VACUUM_SLICE_PAGES, VACUUM_SLICE_BUDGET)
    }

    pub(super) fn with_slice(slice_pages: i64, slice_budget: Duration) -> Self {
        Self {
            owed: 0,
            paused_for_rebuild: false,
            slice_pages,
            slice_budget,
        }
    }

    /// The 30 s tick: top the debt up to this freelist's quota (never ADD a
    /// quota, since ticks pile up behind a backlog), then pay one slice.
    pub(super) fn on_tick(&mut self, conn: &rusqlite::Connection, signal: &IndexFailureSignal) {
        if self.paused_for_rebuild {
            log::debug!("Writer: incremental_vacuum waits for the rebuild, which reuses the free pages");
            return;
        }
        let free = match conn.pragma_query_value(None, "freelist_count", |row| row.get::<_, i64>(0)) {
            Ok(n) => n,
            Err(e) => {
                signal.note_maintenance(&IndexStoreError::from(e), "freelist_count query");
                return;
            }
        };
        let Some(quota) = pick_vacuum_cap(free) else {
            self.owed = 0;
            return;
        };
        self.owed = self.owed.max(quota);
        log::debug!(
            "Writer: incremental_vacuum owes {} of {}",
            pluralize(self.owed as u64, "page"),
            pluralize(free as u64, "free page")
        );
        self.run_slice(conn, signal);
    }

    /// Whether a slice is still owed. The writer asks at its idle point.
    pub(super) fn owes_more(&self) -> bool {
        self.owed > 0
    }

    /// Pay one bounded slice of the debt.
    pub(super) fn run_slice(&mut self, conn: &rusqlite::Connection, signal: &IndexFailureSignal) {
        let ask = self.owed.min(self.slice_pages);
        if ask <= 0 {
            return;
        }
        let started = Instant::now();
        match cmdr_fs::sqlite_util::run_incremental_vacuum_slice(conn, ask, self.slice_budget) {
            // Nothing left to free: the freelist ran out before the quota did.
            Ok(0) => self.owed = 0,
            Ok(freed) => {
                self.owed -= freed as i64;
                log::debug!(
                    "Writer: incremental_vacuum reclaimed {} in {} ms, {} still owed",
                    pluralize(freed, "page"),
                    started.elapsed().as_millis(),
                    pluralize(self.owed.max(0) as u64, "page")
                );
            }
            // Maintenance: the next tick retries (cmdr-reports#105).
            Err(e) => {
                self.owed = 0;
                signal.note_maintenance(&IndexStoreError::from(e), "incremental_vacuum");
            }
        }
    }

    /// `TruncateData` ran: a rebuild follows, and it reuses every free page.
    pub(super) fn pause_for_rebuild(&mut self) {
        self.paused_for_rebuild = true;
        self.owed = 0;
    }

    /// A full aggregate ran, which is how every walk after a truncate ends
    /// (`ComputeAllAggregates`, or `PayLedgerIfUnpaid` for one that stopped).
    pub(super) fn rebuild_settled(&mut self) {
        self.paused_for_rebuild = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::store::{EntryRow, IndexStore, ROOT_ID};
    use crate::indexing::writer::tests::setup_db;
    use crate::indexing::writer::{IndexWriter, WriteMessage};

    // ── DB hygiene tests ─────────────────────────────────────────────

    /// The tier policy is the safety-critical part of the vacuum logic:
    /// regressing it would either thrash the writer lock (cap too aggressive
    /// in steady state) or let the freelist grow unbounded (cap missing on
    /// backlog). Lock the thresholds with explicit cases either side of each
    /// boundary plus the steady-state band's interior.
    #[test]
    fn pick_vacuum_cap_skips_below_min() {
        assert_eq!(pick_vacuum_cap(0), None);
        assert_eq!(pick_vacuum_cap(VACUUM_MIN_FREELIST - 1), None);
    }

    #[test]
    fn pick_vacuum_cap_uses_steady_band_for_modest_backlog() {
        assert_eq!(pick_vacuum_cap(VACUUM_MIN_FREELIST), Some(VACUUM_STEADY_CAP));
        assert_eq!(pick_vacuum_cap(VACUUM_BACKLOG_THRESHOLD - 1), Some(VACUUM_STEADY_CAP));
    }

    #[test]
    fn pick_vacuum_cap_ramps_to_backlog_cap_for_large_backlog() {
        assert_eq!(pick_vacuum_cap(VACUUM_BACKLOG_THRESHOLD), Some(VACUUM_BACKLOG_CAP));
        assert_eq!(pick_vacuum_cap(1_000_000), Some(VACUUM_BACKLOG_CAP));
    }

    // ── Bounded vacuum slices ────────────────────────────────────────

    /// An `auto_vacuum = INCREMENTAL` connection with at least `pages` free pages.
    fn conn_with_freelist(pages: i64) -> rusqlite::Connection {
        let conn = cmdr_fs::sqlite_util::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA auto_vacuum = INCREMENTAL; PRAGMA page_size = 4096;
             CREATE TABLE blobs (id INTEGER PRIMARY KEY, data BLOB);",
        )
        .unwrap();
        let blob = vec![0u8; 4000];
        let mut insert = conn.prepare("INSERT INTO blobs (data) VALUES (?1)").unwrap();
        for _ in 0..pages + 50 {
            insert.execute(rusqlite::params![blob]).unwrap();
        }
        drop(insert);
        conn.execute_batch("DELETE FROM blobs;").unwrap();
        conn
    }

    fn freelist(conn: &rusqlite::Connection) -> i64 {
        conn.pragma_query_value(None, "freelist_count", |row| row.get(0))
            .unwrap()
    }

    /// Drain everything a [`VacuumWork`] still owes, the way the writer's idle
    /// point does, and return how many slices that took.
    fn drain(work: &mut VacuumWork, conn: &rusqlite::Connection, signal: &IndexFailureSignal) -> usize {
        let mut slices = 0;
        while work.owes_more() {
            work.run_slice(conn, signal);
            slices += 1;
            assert!(slices < 10_000, "the vacuum never stopped owing");
        }
        slices
    }

    /// A tick no longer reclaims its whole quota in one go: it runs ONE bounded
    /// slice and owes the rest, which the writer pays in further slices between
    /// messages. The total stays the tick's quota exactly (regression: one 20,000-page
    /// tick held the writer 10.3 s during a rebuild, dev log 2026-11-25).
    #[test]
    fn a_tick_pays_its_quota_in_bounded_slices() {
        let conn = conn_with_freelist(3_000);
        let signal = IndexFailureSignal::new(crate::NoopEventSink::shared());
        let mut work = VacuumWork::with_slice(100, Duration::from_secs(60));
        let before = freelist(&conn);

        work.on_tick(&conn, &signal);
        assert_eq!(before - freelist(&conn), 100, "the tick itself runs one slice");
        assert!(work.owes_more());

        let slices = drain(&mut work, &conn, &signal);
        assert_eq!(slices, 19);
        assert_eq!(
            before - freelist(&conn),
            VACUUM_STEADY_CAP,
            "the slices add up to the tick's quota, no more and no less"
        );
    }

    /// Ticks pile up in the queue behind a long backlog (five ran back to back in
    /// the 2026-11-25 dev log). Each one tops the debt up to its quota rather than
    /// adding another quota, so a backlog doesn't multiply the work.
    #[test]
    fn piled_up_ticks_top_the_debt_up_instead_of_adding_to_it() {
        let conn = conn_with_freelist(6_000);
        let signal = IndexFailureSignal::new(crate::NoopEventSink::shared());
        let mut work = VacuumWork::with_slice(100, Duration::from_secs(60));
        let before = freelist(&conn);

        work.on_tick(&conn, &signal);
        work.on_tick(&conn, &signal);
        work.on_tick(&conn, &signal);
        drain(&mut work, &conn, &signal);

        let reclaimed = before - freelist(&conn);
        assert!(
            reclaimed < 2 * VACUUM_STEADY_CAP,
            "three piled-up ticks must not owe three quotas; reclaimed {reclaimed}"
        );
    }

    /// After a truncate the rebuild refills the freed pages before the file grows,
    /// so vacuuming them mid-walk only moves pages the walk then writes the file
    /// back out over. The tick waits until the rebuild's full aggregate says the
    /// walk is over.
    #[test]
    fn a_rebuild_pauses_the_vacuum_until_its_full_aggregate() {
        let conn = conn_with_freelist(3_000);
        let signal = IndexFailureSignal::new(crate::NoopEventSink::shared());
        let mut work = VacuumWork::with_slice(100, Duration::from_secs(60));
        let before = freelist(&conn);

        work.pause_for_rebuild();
        work.on_tick(&conn, &signal);
        assert_eq!(
            freelist(&conn),
            before,
            "no vacuum while the rebuild refills the freelist"
        );
        assert!(!work.owes_more());

        work.rebuild_settled();
        work.on_tick(&conn, &signal);
        assert_eq!(before - freelist(&conn), 100);
    }

    /// A pause also drops a debt the truncate found half paid: those pages are the
    /// rebuild's to reuse now.
    #[test]
    fn a_truncate_drops_what_the_vacuum_still_owed() {
        let conn = conn_with_freelist(3_000);
        let signal = IndexFailureSignal::new(crate::NoopEventSink::shared());
        let mut work = VacuumWork::with_slice(100, Duration::from_secs(60));
        work.on_tick(&conn, &signal);
        assert!(work.owes_more());

        work.pause_for_rebuild();
        assert!(!work.owes_more());
    }

    /// Through the writer: a `TruncateData` pauses the tick, and the walk's
    /// `ComputeAllAggregates` lifts the pause.
    #[test]
    fn the_writer_holds_the_vacuum_from_a_truncate_to_the_next_full_aggregate() {
        let (db_path, _dir) = setup_db();
        let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
        // Rows pack ~20 a page, so this clears the vacuum's 1,000-page minimum.
        let entries = (0..40_000)
            .map(|i| EntryRow {
                id: 100 + i,
                parent_id: ROOT_ID,
                name: format!("test-entry-with-a-reasonably-long-name-{i:08}"),
                is_directory: false,
                is_symlink: false,
                logical_size: Some(4096),
                physical_size: Some(4096),
                modified_at: None,
                inode: None,
            })
            .collect();
        writer.send(WriteMessage::InsertEntriesV2(entries)).unwrap();
        writer.send(WriteMessage::TruncateData).unwrap();
        writer.flush_blocking().unwrap();
        let probe = IndexStore::open_read_connection(&db_path).unwrap();
        let free_after_truncate = freelist(&probe);
        assert!(
            free_after_truncate >= VACUUM_MIN_FREELIST,
            "test setup: freelist={free_after_truncate}"
        );

        writer.send(WriteMessage::IncrementalVacuum).unwrap();
        writer.flush_blocking().unwrap();
        assert_eq!(freelist(&probe), free_after_truncate, "the tick waits for the rebuild");

        writer
            .send(WriteMessage::ComputeAllAggregates {
                source: crate::indexing::writer::AggSource::Sql,
            })
            .unwrap();
        writer.send(WriteMessage::IncrementalVacuum).unwrap();
        writer.flush_blocking().unwrap();
        assert!(
            freelist(&probe) < free_after_truncate,
            "after the full aggregate the tick vacuums again"
        );

        writer.shutdown();
    }

    /// The capped `incremental_vacuum` handler must reclaim the FULL per-tick
    /// cap, not a single page. `PRAGMA incremental_vacuum(N)` frees one page per
    /// `sqlite3_step()`, so a single `execute_batch` step drains one page
    /// regardless of the cap — which stranded the freelist, draining it one page
    /// per 30 s tick. Build a multi-thousand-page freelist with a subtree delete
    /// (nothing drains it inline), then assert one
    /// `IncrementalVacuum` reclaims far more than a single page.
    #[test]
    fn handle_incremental_vacuum_reclaims_capped_batch() {
        let (db_path, _dir) = setup_db();
        let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();

        // A directory to hang the children off, then a large batch of children.
        // Deleting the subtree frees a few thousand pages onto the freelist with
        // no auto-drain (rows pack ~20/page, so it takes tens of thousands to
        // clear the vacuum's 1 000-page MIN and reach the 2 000-page steady cap).
        let dir_id = 100;
        let mut entries: Vec<EntryRow> = vec![EntryRow {
            id: dir_id,
            parent_id: ROOT_ID,
            name: "subtree".to_string(),
            is_directory: true,
            is_symlink: false,
            logical_size: None,
            physical_size: None,
            modified_at: None,
            inode: None,
        }];
        entries.extend((0..60_000).map(|i| EntryRow {
            id: 101 + i,
            parent_id: dir_id,
            name: format!("test-entry-with-a-reasonably-long-name-{i:08}"),
            is_directory: false,
            is_symlink: false,
            logical_size: Some(4096),
            physical_size: Some(4096),
            modified_at: None,
            inode: None,
        }));
        writer.send(WriteMessage::InsertEntriesV2(entries)).unwrap();
        writer.send(WriteMessage::DeleteSubtreeById(dir_id)).unwrap();
        writer.flush_blocking().unwrap();

        let probe = IndexStore::open_read_connection(&db_path).unwrap();
        let free_before: i64 = probe
            .pragma_query_value(None, "freelist_count", |row| row.get(0))
            .unwrap();
        drop(probe);
        // The freelist must comfortably exceed a single page for the reclaim
        // assertion below to be unambiguous.
        assert!(
            free_before > 1_000,
            "test setup: expected a multi-thousand-page freelist, got {free_before}"
        );

        writer.send(WriteMessage::IncrementalVacuum).unwrap();
        writer.flush_blocking().unwrap();

        let probe = IndexStore::open_read_connection(&db_path).unwrap();
        let free_after: i64 = probe
            .pragma_query_value(None, "freelist_count", |row| row.get(0))
            .unwrap();

        let reclaimed = free_before - free_after;
        assert!(
            reclaimed >= 1_000,
            "one IncrementalVacuum must reclaim the per-tick cap, not a single page; \
             before={free_before}, after={free_after}, reclaimed={reclaimed}"
        );

        writer.shutdown();
    }

    /// Regression (#1296): `TruncateData` leaves its freed pages on the freelist for
    /// the rebuild to refill, ❌ never drains them inline (an uncapped drain held the
    /// writer ~63 s on a 7M-row index, only to grow the file straight back). The
    /// rebuild reuses them, so the file doesn't grow past its pre-truncate size.
    #[test]
    fn truncate_leaves_its_pages_for_the_rebuild_to_reuse() {
        let (db_path, _dir) = setup_db();
        let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();

        // Long names so 5000 rows span thousands of pages.
        let entries = || -> Vec<EntryRow> {
            (0..5000)
                .map(|i| EntryRow {
                    id: 100 + i,
                    parent_id: ROOT_ID,
                    name: format!("test-entry-with-a-reasonably-long-name-{i:08}"),
                    is_directory: false,
                    is_symlink: false,
                    logical_size: Some(4096),
                    physical_size: Some(4096),
                    modified_at: None,
                    inode: None,
                })
                .collect()
        };
        let probe = IndexStore::open_read_connection(&db_path).unwrap();
        let pragma = |name: &str| -> i64 { probe.pragma_query_value(None, name, |row| row.get(0)).unwrap() };

        writer.send(WriteMessage::InsertEntriesV2(entries())).unwrap();
        writer.flush_blocking().unwrap();
        let pages_before = pragma("page_count");

        writer.send(WriteMessage::TruncateData).unwrap();
        writer.flush_blocking().unwrap();
        let freed = pragma("freelist_count");
        assert!(freed >= 100, "the truncate must leave its pages free; freelist={freed}");

        writer.send(WriteMessage::InsertEntriesV2(entries())).unwrap();
        writer.flush_blocking().unwrap();
        let pages_after = pragma("page_count");
        assert!(
            pages_after <= pages_before + 10,
            "the rebuild must reuse the freed pages; before={pages_before}, after={pages_after}"
        );

        writer.shutdown();
    }
}
