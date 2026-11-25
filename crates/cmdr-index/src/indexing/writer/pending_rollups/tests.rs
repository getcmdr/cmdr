//! The roll-up queue on its own: coalescing, the yield's cost, and a chain that
//! can't be read. The writer-loop tests (transaction, re-queue, live changes in
//! between) are in `../aggregation/rollup_tests.rs`.

use super::*;
use crate::indexing::stress_test_helpers::check_db_consistency;
use crate::indexing::writer::entry_fixtures::{dir_entry, file_entry};
use crate::indexing::writer::tests::{settle_the_writer, setup_db};
use crate::indexing::writer::{AggSource, IndexWriter, WriteMessage};

fn rollups() -> PendingRollups {
    PendingRollups::new("test-volume", Arc::new(RollupGauges::default()))
}

/// The whole point of the set: a burst of roots sharing one parent owes ONE
/// recompute, not one per root.
#[test]
fn a_burst_sharing_one_parent_queues_one_id() {
    let rollups = rollups();
    for _ in 0..10_000 {
        rollups.queue(42);
    }
    assert_eq!(rollups.owed_count(), 1, "one parent, one queued roll-up");
}

/// Distinct parents each keep their own entry: coalescing must never lose a
/// chain that nothing else is going to repair.
#[test]
fn distinct_parents_all_survive() {
    let rollups = rollups();
    for id in 1..=50 {
        rollups.queue(id);
    }
    assert_eq!(rollups.owed_count(), 50);
}

/// `ROOT` → `big`(10) → `width` mids, each with a sub holding one 1,000-byte file,
/// aggregated, then the mids, `big`, and the root drifted to zero bytes: every
/// one of them owes a recompute, and only a roll-up puts them back. Returns the
/// write connection (the writer is gone) and the mids.
fn drifted_tree(width: i64) -> (rusqlite::Connection, Vec<i64>, tempfile::TempDir) {
    let (db_path, dir) = setup_db();
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).unwrap();
    let mut entries = vec![dir_entry(10, ROOT_ID, "big")];
    let mids: Vec<i64> = (0..width).map(|index| 100 + index * 3).collect();
    for &mid in &mids {
        entries.push(dir_entry(mid, 10, &format!("mid-{mid}")));
        entries.push(dir_entry(mid + 1, mid, "sub"));
        entries.push(file_entry(mid + 2, mid + 1, "leaf", 1000));
    }
    writer.send(WriteMessage::InsertEntriesV2(entries)).unwrap();
    writer
        .send(WriteMessage::ComputeAllAggregates {
            source: AggSource::Maps,
        })
        .unwrap();
    settle_the_writer(&writer);
    writer.shutdown();

    let conn = IndexStore::open_write_connection(&db_path).unwrap();
    conn.execute(
        "UPDATE dir_stats SET recursive_logical_size = 0
         WHERE entry_id IN (SELECT id FROM entries WHERE parent_id = 10) OR entry_id IN (10, ?1)",
        [ROOT_ID],
    )
    .unwrap();
    (conn, mids, dir)
}

fn logical(conn: &rusqlite::Connection, id: i64) -> u64 {
    IndexStore::get_dir_stats_by_id(conn, id)
        .unwrap()
        .unwrap()
        .recursive_logical_size
}

/// **The yield must not re-read what it knew.** A message always waiting and a
/// slice too short for more than one recompute: the drain yields after every
/// folder. Throwing the chains away on each yield re-read one or two parents per
/// owed id per drain, `O(owed²)` reads (the PR #1485 review). The heap and the
/// depths survive the yield, so the reads stay linear in the folders owed, and
/// everything owed still settles, each folder once.
#[test]
fn a_drain_that_yields_every_folder_reads_each_parent_about_once() {
    const WIDTH: i64 = 200;
    let (conn, mids, _dir) = drifted_tree(WIDTH);
    let rollups = rollups();
    rollups.gauges.set_slice_for_test(Duration::from_nanos(1));
    for &mid in &mids {
        rollups.queue(mid);
    }
    let repairs = DeferredRepairs::new();

    let mut drains = 0;
    while !rollups.is_empty() {
        let drained = rollups.drain(&conn, &repairs, &|| true);
        drains += 1;
        assert!(drains <= WIDTH + 2, "every drain recomputes at least one folder");
        if !rollups.is_empty() {
            assert!(drained.owes_more, "a drain that left work owed says so");
        }
    }

    let owed = WIDTH as u64 + 2; // the mids, `big`, the root
    let gauges = &rollups.gauges;
    assert_eq!(gauges.recomputes.load(Ordering::Relaxed), owed, "each folder once");
    let reads = gauges.parent_reads.load(Ordering::Relaxed);
    assert!(
        reads <= 3 * owed,
        "{} for {}: a yield is re-reading chains it already read",
        pluralize(reads, "parent read"),
        pluralize(owed, "owed folder"),
    );
    assert!(
        gauges.yields.load(Ordering::Relaxed) >= WIDTH as u64,
        "it really did yield every time"
    );
    assert_eq!(gauges.owed.load(Ordering::Relaxed), 0);
    assert_eq!(logical(&conn, 10), WIDTH as u64 * 1000);
    assert_eq!(logical(&conn, ROOT_ID), WIDTH as u64 * 1000);
    check_db_consistency(&conn);
}

/// **A chain that can't be read never drops the queued folder.** Its ancestor's
/// parent read fails, so the depth is unknown: the folder is still owed, its
/// rewrite still owes its parent, and the ancestor whose parent read failed waits
/// for the next drain, which finishes the chain once the read works again.
#[test]
fn an_unreadable_chain_still_rolls_the_queued_folder_and_its_ancestors_up() {
    let (conn, mids, _dir) = drifted_tree(1);
    let mid = mids[0];
    let rollups = rollups();
    let repairs = DeferredRepairs::new();

    // `big`'s parent can't be read: `mid`'s depth walk breaks one level up.
    rollups.fail_parent_read_of.set(Some(10));
    rollups.queue(mid);
    rollups.drain(&conn, &repairs, &|| false);
    assert_eq!(logical(&conn, mid), 1000, "the queued folder rolled up anyway");
    assert_eq!(logical(&conn, 10), 1000, "and so did the parent its rewrite owed");
    assert_eq!(
        logical(&conn, ROOT_ID),
        0,
        "the root waits on the parent read that failed"
    );
    assert!(!rollups.is_empty(), "the failed read is still owed, never dropped");

    rollups.fail_parent_read_of.set(None);
    rollups.drain(&conn, &repairs, &|| false);
    assert!(rollups.is_empty());
    assert_eq!(logical(&conn, ROOT_ID), 1000, "the next drain finished the chain");
    check_db_consistency(&conn);
}

/// The queued folder's OWN parent read failing is the same story one level down.
#[test]
fn a_queued_folder_whose_parent_read_fails_waits_for_it() {
    let (conn, mids, _dir) = drifted_tree(1);
    let mid = mids[0];
    let rollups = rollups();
    let repairs = DeferredRepairs::new();

    rollups.fail_parent_read_of.set(Some(mid));
    rollups.queue(mid);
    rollups.drain(&conn, &repairs, &|| false);
    assert_eq!(logical(&conn, mid), 1000);
    assert_eq!(logical(&conn, 10), 0, "the parent is owed, not yet known");
    assert!(!rollups.is_empty());

    rollups.fail_parent_read_of.set(None);
    rollups.drain(&conn, &repairs, &|| false);
    assert!(rollups.is_empty());
    assert_eq!(logical(&conn, ROOT_ID), 1000);
    check_db_consistency(&conn);
}
