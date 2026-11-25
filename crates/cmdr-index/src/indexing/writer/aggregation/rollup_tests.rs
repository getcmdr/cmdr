//! Tests for the per-burst ancestor roll-up ([`super::super::pending_rollups`]).
//!
//! A walk stopped after a wide directory was listed leaves every unwalked child
//! of it a frontier root of its own, and each root's walk ends in one
//! `ComputeSubtreeAggregates`. Rolling the shared parent up per message made that
//! `O(width²)` and took a 60,000-child directory to 73 minutes
//! (`docs/notes/wide-dir-scaling-2026-08-18.md`). These pin the fix: the burst
//! costs ONE roll-up, the answer it lands is the same one the per-message version
//! landed, and a quit inside a burst still pays what the burst owes.

use std::sync::atomic::Ordering;

use cmdr_fs::pluralize::pluralize;

use super::tests::{dir_row, file_row, set_epoch};
use crate::indexing::store::{IndexStore, ROOT_ID};
use crate::indexing::stress_test_helpers::check_db_consistency;
use crate::indexing::writer::tests::{settle_the_writer, setup_db};
use crate::indexing::writer::{AggSource, IndexWriter, WriteMessage};

/// One parent holding `width` child dirs, each with one file of `size` bytes.
/// The shape a stopped walk of a wide directory leaves behind.
fn wide_parent(writer: &IndexWriter, width: i64, size: u64) {
    let mut entries = vec![dir_row(10, ROOT_ID, "big")];
    let mut listed = vec![ROOT_ID, 10];
    for index in 0..width {
        let dir_id = 100 + index * 2;
        entries.push(dir_row(dir_id, 10, &format!("sub-{index}")));
        entries.push(file_row(dir_id + 1, dir_id, "leaf", size));
        listed.push(dir_id);
    }
    writer.send(WriteMessage::InsertEntriesV2(entries)).unwrap();
    writer
        .send(WriteMessage::MarkDirsListed { ids: listed, epoch: 1 })
        .unwrap();
    writer
        .send(WriteMessage::ComputeAllAggregates {
            source: AggSource::Maps,
        })
        .unwrap();
    writer.flush_blocking().unwrap();
}

/// **The scaling guard.** `width` frontier roots sharing one parent must cost a
/// HANDFUL of ancestor walks, not one each — that ratio IS the difference between
/// linear and quadratic, and it's what a well-meaning "repair it right here, it's
/// race-free that way" would put back.
///
/// Reads `rollup_recomputes`, the writer's own count of drained roll-ups, so the
/// assertion is about the mechanism rather than about a stopwatch. Pre-fix this
/// number was exactly `width`.
#[test]
fn a_burst_of_roots_under_one_parent_costs_a_handful_of_rollups() {
    const WIDTH: i64 = 400;

    let (db_path, _dir) = setup_db();
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
    wide_parent(&writer, WIDTH, 1000);
    // Blank the ancestors' coverage the way a live new-dir upsert does, so the
    // assertions below can only pass if a roll-up really ran: without this the
    // rows are already right and a skipped drain would read as a pass.
    set_epoch(&db_path, 10, 0);
    set_epoch(&db_path, ROOT_ID, 0);

    // Every child's walk finishes and reports, back to back, the way the phase
    // machine's drain does — nothing flushes in between, so the writer stays
    // behind and the burst is one burst.
    for index in 0..WIDTH {
        writer
            .send(WriteMessage::ComputeSubtreeAggregates {
                root_id: 100 + index * 2,
            })
            .unwrap();
    }
    settle_the_writer(&writer);

    let walks = writer.rollup_recomputes();
    assert!(
        walks <= (WIDTH as u64) / 10,
        "{} sharing one parent cost {}; the burst must coalesce, \
         or a wide directory is quadratic again",
        pluralize(WIDTH as u64, "frontier root"),
        pluralize(walks, "ancestor roll-up"),
    );

    // And the coalescing changed no answer.
    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    let big = IndexStore::get_dir_stats_by_id(&conn, 10).unwrap().unwrap();
    assert_eq!(
        (
            big.recursive_logical_size,
            big.recursive_file_count,
            big.recursive_dir_count
        ),
        (WIDTH as u64 * 1000, WIDTH as u64, WIDTH as u64),
    );
    assert_eq!(big.min_subtree_epoch, 1, "the one roll-up restored the coverage");
    let root = IndexStore::get_dir_stats_by_id(&conn, ROOT_ID).unwrap().unwrap();
    assert_eq!(root.min_subtree_epoch, 1, "and carried it the rest of the way up");
    check_db_consistency(&conn);

    writer.shutdown();
}

/// A live mutation landing INSIDE the window where the roll-up is still owed must
/// not be double-counted or lost. The roll-up recomputes from committed children
/// rather than adding a delta, so the later it runs the more of the truth it sees
/// — this is the whole race argument, asserted.
#[test]
fn a_mutation_inside_the_pending_window_is_counted_once() {
    let (db_path, _dir) = setup_db();
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
    wide_parent(&writer, 4, 1000);

    // Two roots report, then somebody's editor writes a file straight into the
    // wide directory, then the rest report. The roll-up is owed throughout.
    for index in 0..2 {
        writer
            .send(WriteMessage::ComputeSubtreeAggregates {
                root_id: 100 + index * 2,
            })
            .unwrap();
    }
    writer
        .send(WriteMessage::UpsertEntryV2 {
            parent_id: 10,
            name: "typed-just-now.txt".to_string(),
            is_directory: false,
            is_symlink: false,
            logical_size: Some(7),
            physical_size: Some(7),
            modified_at: None,
            inode: None,
            nlink: None,
        })
        .unwrap();
    for index in 2..4 {
        writer
            .send(WriteMessage::ComputeSubtreeAggregates {
                root_id: 100 + index * 2,
            })
            .unwrap();
    }
    settle_the_writer(&writer);

    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    let big = IndexStore::get_dir_stats_by_id(&conn, 10).unwrap().unwrap();
    assert_eq!(
        (
            big.recursive_logical_size,
            big.recursive_file_count,
            big.recursive_dir_count
        ),
        (4 * 1000 + 7, 5, 4),
        "the new file counts exactly once, whichever side of the roll-up it landed on"
    );
    let root = IndexStore::get_dir_stats_by_id(&conn, ROOT_ID).unwrap().unwrap();
    assert_eq!(root.recursive_logical_size, 4 * 1000 + 7);
    check_db_consistency(&conn);

    writer.shutdown();
}

/// A quit landing inside a burst still pays what the burst owes. `shutdown` runs
/// the same settle the caught-up point does, so the ancestors are right on the
/// next launch instead of waiting for the run to complete.
#[test]
fn a_shutdown_inside_a_burst_still_rolls_the_ancestors_up() {
    let (db_path, _dir) = setup_db();
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
    wide_parent(&writer, 6, 1000);

    // Blank the ancestors the way a live new-dir upsert does, so only a roll-up
    // can put them back.
    set_epoch(&db_path, 10, 0);
    set_epoch(&db_path, ROOT_ID, 0);

    for index in 0..6 {
        writer
            .send(WriteMessage::ComputeSubtreeAggregates {
                root_id: 100 + index * 2,
            })
            .unwrap();
    }
    // ❌ No settle: the quit is what has to pay.
    writer.shutdown();

    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    let big = IndexStore::get_dir_stats_by_id(&conn, 10).unwrap().unwrap();
    assert_eq!(big.min_subtree_epoch, 1, "the quit rolled the coverage back up");
    let root = IndexStore::get_dir_stats_by_id(&conn, ROOT_ID).unwrap().unwrap();
    assert_eq!(root.min_subtree_epoch, 1);
    check_db_consistency(&conn);
}

/// A truncating rescan wipes the rows a queued roll-up names, so the queue has to
/// go with them — the same reason `TruncateData` clears the deferred repairs.
///
/// Without that, the drain recomputes a directory that no longer exists, finds no
/// children, and writes a zeroed `dir_stats` row for a deleted entry id. The id
/// counter resets on truncate, so that ghost row then belongs to whatever the next
/// scan puts at the same id.
#[test]
fn a_truncate_drops_the_roll_ups_it_made_meaningless() {
    let (db_path, _dir) = setup_db();
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
    wide_parent(&writer, 2, 1000);

    // A root reports, and the truncating rescan lands behind it before the writer
    // has caught up — so the roll-up for id 10 is still owed when the tables go.
    writer
        .send(WriteMessage::ComputeSubtreeAggregates { root_id: 100 })
        .unwrap();
    writer.send(WriteMessage::TruncateData).unwrap();
    settle_the_writer(&writer);

    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    let ghosts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM dir_stats ds LEFT JOIN entries e ON e.id = ds.entry_id WHERE e.id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ghosts, 0, "a truncate must leave no dir_stats row without an entry");

    writer.shutdown();
}

/// The same hazard without a truncate: the reconciler reaps a directory while the
/// roll-up its child's scan queued is still owed. The drain must not resurrect it
/// as a zeroed row.
///
/// A delete already walks its own debit up the chain, so there is nothing left for
/// the roll-up to do here — skipping it is correct as well as safe.
#[test]
fn a_delete_inside_the_pending_window_leaves_no_ghost_row() {
    let (db_path, _dir) = setup_db();
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
    wide_parent(&writer, 2, 1000);

    writer
        .send(WriteMessage::ComputeSubtreeAggregates { root_id: 100 })
        .unwrap();
    writer.send(WriteMessage::DeleteSubtreeById(10)).unwrap();
    settle_the_writer(&writer);

    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    let ghosts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM dir_stats ds LEFT JOIN entries e ON e.id = ds.entry_id WHERE e.id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        ghosts, 0,
        "a reaped directory must not come back as a zeroed dir_stats row"
    );
    let root = IndexStore::get_dir_stats_by_id(&conn, ROOT_ID).unwrap().unwrap();
    assert_eq!(
        (root.recursive_logical_size, root.recursive_file_count),
        (0, 0),
        "and the delete's own debit still reached the root"
    );
    check_db_consistency(&conn);

    writer.shutdown();
}

/// One wide directory whose `width` children each get ONE freshly scanned
/// frontier root: `big` → `mid-i` → `sub-i` → a 1,000-byte file. The shape a phase
/// that stops one level below a wide directory leaves behind (`C:\Windows\WinSxS`,
/// a `node_modules`).
///
/// `big` and the mids are scanned and aggregated first; the subs then arrive the
/// way a cover walk sends them (`InsertSubtreeEntries`, which credits no
/// ancestor), so every mid, `big`, and the root are short by exactly what the subs
/// hold. Only the roll-up puts them right. Returns the scan to send.
fn wide_grandparent(writer: &IndexWriter, width: i64) -> Vec<WriteMessage> {
    let mut parents = vec![dir_row(10, ROOT_ID, "big")];
    let mut listed = vec![ROOT_ID, 10];
    for index in 0..width {
        let mid = 100 + index * 3;
        parents.push(dir_row(mid, 10, &format!("mid-{index}")));
        listed.push(mid);
    }
    writer.send(WriteMessage::InsertEntriesV2(parents)).unwrap();
    writer
        .send(WriteMessage::MarkDirsListed { ids: listed, epoch: 1 })
        .unwrap();
    writer
        .send(WriteMessage::ComputeAllAggregates {
            source: AggSource::Maps,
        })
        .unwrap();
    settle_the_writer(writer);

    let mut subs = Vec::new();
    let mut scanned = Vec::new();
    for index in 0..width {
        let mid = 100 + index * 3;
        scanned.push(dir_row(mid + 1, mid, "sub"));
        scanned.push(file_row(mid + 2, mid + 1, "leaf", 1000));
        subs.push(mid + 1);
    }
    let mut scan = vec![
        WriteMessage::InsertSubtreeEntries(scanned),
        WriteMessage::MarkDirsListed {
            ids: subs.clone(),
            epoch: 1,
        },
    ];
    scan.extend(
        subs.into_iter()
            .map(|root_id| WriteMessage::ComputeSubtreeAggregates { root_id }),
    );
    scan
}

/// Send `scan` as ONE burst: inside an explicit transaction the writer can't
/// settle, so every roll-up it queues is owed to the same drain, however the
/// writer keeps up with the sends.
fn send_as_one_burst(writer: &IndexWriter, scan: Vec<WriteMessage>) {
    let transaction = writer.begin_transaction().unwrap();
    for message in scan {
        writer.send(message).unwrap();
    }
    transaction.commit().unwrap();
}

/// Every file's bytes, summed straight from `entries`: what the root must say.
fn bytes_on_disk(conn: &rusqlite::Connection) -> u64 {
    conn.query_row(
        "SELECT COALESCE(SUM(logical_size), 0) FROM entries WHERE is_directory = 0",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

/// **The GitHub issue #1480 guard.** Frontier roots one level BELOW a wide
/// directory each queue a DIFFERENT parent (`mid-i`), so the set can't coalesce
/// them, and every one of those chains runs through `big`. Walking each chain to
/// the root recomputed `big` from all `width` of its children once per chain:
/// `O(width²)` again, one level up from the case above. A stopped first index of
/// a Windows `C:` (WinSxS: 18,384 component folders, 5,864 of them holding `f`
/// and `r`) held the writer in one drain for over an hour. Pre-fix this cost 601.
///
/// The drain must recompute each directory ONCE, children before parents: `width`
/// mids, then `big`, then the root.
#[test]
fn roots_under_a_wide_grandparent_recompute_it_once() {
    const WIDTH: i64 = 300;

    let (db_path, _dir) = setup_db();
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
    let scan = wide_grandparent(&writer, WIDTH);
    let before = writer.rollup_recomputes();

    send_as_one_burst(&writer, scan);
    settle_the_writer(&writer);

    let recomputes = writer.rollup_recomputes() - before;
    assert_eq!(
        recomputes,
        WIDTH as u64 + 2,
        "{} under one wide grandparent: each directory must be recomputed once \
         (the mids, then `big`, then the root), or a wide directory is quadratic again",
        pluralize(WIDTH as u64, "frontier root"),
    );

    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    let big = IndexStore::get_dir_stats_by_id(&conn, 10).unwrap().unwrap();
    assert_eq!(
        (
            big.recursive_logical_size,
            big.recursive_file_count,
            big.recursive_dir_count,
            big.min_subtree_epoch
        ),
        (WIDTH as u64 * 1000, WIDTH as u64, WIDTH as u64 * 2, 1),
        "one recompute per level still lands the whole truth, coverage included"
    );
    let root = IndexStore::get_dir_stats_by_id(&conn, ROOT_ID).unwrap().unwrap();
    assert_eq!(
        root.recursive_logical_size,
        WIDTH as u64 * 1000,
        "the sizes reached the root"
    );
    assert_eq!(root.min_subtree_epoch, 1, "and so did the coverage");
    check_db_consistency(&conn);

    writer.shutdown();
}

/// **Sliced, interrupted, and rolled back, the ledger still ends exact.** Through
/// the real writer loop: every drain yields after one folder, the first two
/// drains' transactions fail to commit (so their recomputes are owed again), and
/// live deletes, adds, and a move land between the slices. Once nothing is owed,
/// every folder's stored sizes must equal a from-scratch computation
/// (`check_db_consistency`), and the root must hold exactly the bytes on disk.
#[test]
fn a_sliced_drain_with_live_changes_between_the_slices_ends_exact() {
    const WIDTH: i64 = 40;

    let (db_path, _dir) = setup_db();
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
    let scan = wide_grandparent(&writer, WIDTH);
    let gauges = writer.rollup_gauges();
    gauges.set_slice_for_test(std::time::Duration::from_nanos(1));
    gauges.yield_always.store(true, Ordering::Relaxed);
    gauges.fail_commits.store(2, Ordering::Relaxed);

    send_as_one_burst(&writer, scan);
    let mid = |index: i64| 100 + index * 3;
    let live = vec![
        // A reaped subtree, a file typed into a mid, a new folder under `big`.
        WriteMessage::DeleteSubtreeById(mid(0) + 1),
        WriteMessage::UpsertEntryV2 {
            parent_id: mid(1),
            name: "typed-just-now.txt".to_string(),
            is_directory: false,
            is_symlink: false,
            logical_size: Some(7),
            physical_size: Some(7),
            modified_at: None,
            inode: None,
            nlink: None,
        },
        WriteMessage::UpsertEntryV2 {
            parent_id: 10,
            name: "new-folder".to_string(),
            is_directory: true,
            is_symlink: false,
            logical_size: None,
            physical_size: None,
            modified_at: None,
            inode: None,
            nlink: None,
        },
        // A scanned sub moves to another mid while both still owe a roll-up.
        WriteMessage::MoveEntryV2 {
            entry_id: mid(2) + 1,
            new_parent_id: mid(3),
            new_name: "moved-sub".to_string(),
        },
        // And a file goes away.
        WriteMessage::DeleteEntryById(mid(4) + 2),
    ];
    for message in live {
        writer.send(message).unwrap();
        // Each flush is an iteration, so each one ends in a one-folder slice.
        writer.flush_blocking().unwrap();
    }
    assert!(
        gauges.owed.load(Ordering::Relaxed) > 0,
        "the live changes landed mid-drain"
    );

    // Keep the slices coming until nothing is owed.
    let mut flushes = 0;
    while gauges.owed.load(Ordering::Relaxed) > 0 {
        writer.flush_blocking().unwrap();
        flushes += 1;
        assert!(flushes <= 20 * WIDTH, "every slice makes progress");
    }
    gauges.yield_always.store(false, Ordering::Relaxed);
    settle_the_writer(&writer);

    assert_eq!(
        gauges.fail_commits.load(Ordering::Relaxed),
        0,
        "both failed commits ran"
    );
    assert!(
        gauges.yields.load(Ordering::Relaxed) >= WIDTH as u64,
        "the drain really was sliced"
    );
    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    check_db_consistency(&conn);
    let root = IndexStore::get_dir_stats_by_id(&conn, ROOT_ID).unwrap().unwrap();
    assert_eq!(root.recursive_logical_size, bytes_on_disk(&conn));

    writer.shutdown();
}

/// Every directory's stored sizes and counts, keyed by id.
fn stored_dir_stats(conn: &rusqlite::Connection) -> std::collections::BTreeMap<i64, (u64, u64, u64, u64)> {
    let mut stmt = conn
        .prepare(
            "SELECT entry_id, recursive_logical_size, recursive_physical_size, recursive_file_count, recursive_dir_count
             FROM dir_stats",
        )
        .unwrap();
    stmt.query_map([], |row| {
        Ok((row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

/// **The real-machine drift audit** (`DETAILS.md` § "Measuring a first index on
/// the real machine"). Point `CMDR_DRIFT_DB` at a COPY of a drive index taken
/// after Cmdr quit (`index-root.db`, plus its `-wal` if one is left), and run
/// `cargo nextest run -p cmdr-index drift_audit --run-ignored only --no-capture`.
/// It copies the copy again, recomputes every `dir_stats` row from `entries` with
/// a full aggregate, and fails listing each folder whose stored sizes differ. A
/// pass means the roll-ups left the ledger exact.
#[test]
#[ignore = "a tool: needs CMDR_DRIFT_DB pointing at a copied index"]
fn drift_audit_against_a_copied_index() {
    let Ok(source) = std::env::var("CMDR_DRIFT_DB") else {
        panic!("set CMDR_DRIFT_DB to a copied index-<volume>.db");
    };
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("audit.db");
    std::fs::copy(&source, &copy).unwrap();
    for suffix in ["-wal", "-shm"] {
        let side = format!("{source}{suffix}");
        if std::path::Path::new(&side).exists() {
            std::fs::copy(&side, format!("{}{suffix}", copy.display())).unwrap();
        }
    }

    let stored = stored_dir_stats(&IndexStore::open_read_connection(&copy).unwrap());
    let writer = IndexWriter::spawn(&copy, crate::NoopEventSink::shared()).unwrap();
    writer
        .send(WriteMessage::ComputeAllAggregates { source: AggSource::Sql })
        .unwrap();
    settle_the_writer(&writer);
    writer.shutdown();
    let fresh = stored_dir_stats(&IndexStore::open_read_connection(&copy).unwrap());

    let drift: Vec<String> = fresh
        .iter()
        .filter(|(id, truth)| stored.get(id) != Some(truth))
        .map(|(id, truth)| format!("id={id}: stored {:?}, recomputed {truth:?}", stored.get(id)))
        .collect();
    assert!(
        drift.is_empty(),
        "{} of {} drifted (logical, physical, files, dirs):\n{}",
        pluralize(drift.len() as u64, "folder"),
        fresh.len(),
        drift.join("\n"),
    );
}
