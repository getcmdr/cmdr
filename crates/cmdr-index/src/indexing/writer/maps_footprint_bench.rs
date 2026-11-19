//! How much heap the writer's [`AccumulatorMaps`] hold for a real drive's rows.
//!
//! A measurement harness, run by hand, never in CI. It feeds every row of an
//! existing index database through [`AccumulatorMaps::accumulate`], which is
//! exactly what the maps hold after a walk that inserted every row through a
//! map-feeding insert, and reports the bytes they still hold. Point it at a
//! read-only copy or the live dev index (it opens the file read-only):
//!
//! ```sh
//! CMDR_MAPS_BENCH_DB="$HOME/Library/Caches/com.veszelovszki.cmdr-dev/drive-index/index-root.db" \
//!   cargo nextest run -p cmdr-index --release --run-ignored only --no-capture maps_footprint
//! ```
//!
//! The number is requested bytes (`heap_bytes_held`), a floor on real residency.
//! Results and the call they backed: `writer/DETAILS.md` § "Only a fresh full scan
//! feeds the maps".

use super::AccumulatorMaps;
use crate::indexing::store::EntryRow;
use crate::indexing::test_support::heap_bytes_held;

/// Rows per accumulate call, the same shape a walk's `InsertEntriesV2` batch has.
const BATCH: usize = 2000;

#[test]
#[ignore = "measurement over a real index DB; set CMDR_MAPS_BENCH_DB and run with --no-capture"]
#[allow(clippy::print_stdout, reason = "a benchmark prints its measurements")]
fn maps_footprint_for_a_real_index() {
    let path: std::path::PathBuf = std::env::var_os("CMDR_MAPS_BENCH_DB")
        .expect("set CMDR_MAPS_BENCH_DB to an index DB path")
        .into();
    let conn = cmdr_fs::sqlite_util::open_read_only(&path).expect("open the index read-only");

    let (maps, held) = heap_bytes_held(|| {
        let mut maps = AccumulatorMaps::new();
        let mut stmt = conn
            .prepare("SELECT id, parent_id, is_directory, is_symlink, logical_size, physical_size FROM entries")
            .expect("prepare");
        let mut rows = stmt.query([]).expect("query");
        let mut batch: Vec<EntryRow> = Vec::with_capacity(BATCH);
        while let Some(row) = rows.next().expect("step") {
            batch.push(EntryRow {
                id: row.get(0).expect("id"),
                parent_id: row.get(1).expect("parent_id"),
                name: String::new(),
                is_directory: row.get(2).expect("is_directory"),
                is_symlink: row.get(3).expect("is_symlink"),
                logical_size: row.get(4).expect("logical_size"),
                physical_size: row.get(5).expect("physical_size"),
                modified_at: None,
                inode: None,
            });
            if batch.len() == BATCH {
                maps.accumulate(&batch);
                batch.clear();
            }
        }
        maps.accumulate(&batch);
        drop(batch);
        maps
    });

    println!(
        "AccumulatorMaps for {} rows: {} direct_stats parents, {} child_dirs parents, {:.1} MB held",
        maps.entries_inserted,
        maps.direct_stats.len(),
        maps.child_dirs.len(),
        held as f64 / 1_000_000.0,
    );
}
