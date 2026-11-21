//! Only a whole-volume walk feeds the writer's accumulator maps.
//!
//! The maps exist for one reader: the `ComputeAllAggregates { source: Maps }` a
//! fresh full scan ends with. A subtree walk (`scan_subtree`, a verifier's
//! rebuild) and a cover walk (`cover_subtree`, the phase machine's) end in
//! `ComputeSubtreeAggregates`, which never reads them, so rows fed from those
//! walks sat in memory until the next full aggregate or truncate: 81 MB after a
//! phased first index of a 7.2M-entry boot volume (#1296).
//!
//! The maps live on the writer thread, so these tests watch them through the one
//! reader that would use them: a `Maps` aggregate rolls up from whatever the maps
//! hold, and falls back to the committed rows only when they're empty. Rows a
//! subtree walk fed in would roll the walk root's ancestors up from nothing.

use super::test_fixtures::{ensure_path_in_db, scan_test_tempdir, setup_writer};
use super::*;
use crate::indexing::paths::app_text::native_text;
use crate::indexing::store::{self, IndexStore};
use crate::indexing::writer::tests::settle_the_writer;
use std::fs;

/// Walk `walked/` (30 bytes in two files) with `walk`, then run a `Maps` full
/// aggregate and report the recursive size of the walk root's parent.
fn parent_size_after_a_maps_aggregate(walk: impl FnOnce(&Path, &IndexWriter)) -> u64 {
    let dir = scan_test_tempdir();
    let walked = dir.path().join("walked");
    fs::create_dir_all(&walked).unwrap();
    fs::write(walked.join("a.txt"), "0123456789").unwrap();
    fs::write(walked.join("b.txt"), "01234567890123456789").unwrap();

    let (writer, db_path, _db_dir) = setup_writer();
    ensure_path_in_db(&db_path, &walked, &writer);
    walk(&walked, &writer);
    settle_the_writer(&writer);

    writer
        .send(WriteMessage::ComputeAllAggregates {
            source: AggSource::Maps,
        })
        .unwrap();
    settle_the_writer(&writer);

    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    let parent_id = store::resolve_path(&conn, &native_text(dir.path()))
        .unwrap()
        .expect("the parent row exists");
    let size = IndexStore::get_dir_stats_by_id(&conn, parent_id)
        .unwrap()
        .expect("the parent has dir_stats")
        .recursive_logical_size;
    writer.shutdown();
    size
}

#[test]
fn a_cover_walk_leaves_the_accumulator_maps_empty() {
    let size = parent_size_after_a_maps_aggregate(|root, writer| {
        cover_subtree(
            root,
            &IndexPathSpace::root(),
            writer,
            None,
            &VolumeWork::for_test("maps-feed-test"),
            &WalkHeartbeat::new(),
        )
        .expect("the cover walk runs");
    });
    assert_eq!(
        size, 30,
        "the maps held the cover walk's rows, so the aggregate read them"
    );
}

#[test]
fn a_subtree_rebuild_leaves_the_accumulator_maps_empty() {
    let size = parent_size_after_a_maps_aggregate(|root, writer| {
        scan_subtree(
            root,
            &IndexPathSpace::root(),
            writer,
            &VolumeWork::for_test("maps-feed-test"),
        )
        .expect("the rebuild runs");
    });
    assert_eq!(size, 30, "the maps held the rebuild's rows, so the aggregate read them");
}
