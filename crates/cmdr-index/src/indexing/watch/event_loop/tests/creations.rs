//! Directory creations through `process_live_batch`: a tree created inside one
//! batch lands as rows, level by level, instead of waiting on a rescan.

use super::rename::{insert_path_chain, rename_test_setup, rename_test_tempdir, run_live_batch};
use super::*;
use crate::indexing::paths::app_text::native_text;
use std::collections::HashMap;

use crate::indexing::reconcile::reconciler::EventReconciler;

fn created_dir() -> watcher::FsEventFlags {
    watcher::FsEventFlags {
        item_created: true,
        item_is_dir: true,
        ..Default::default()
    }
}

/// `mkdir -p tree/a/b` inside one batch puts all three folders in the index, so a
/// listing of `tree/a` resolves straight away.
///
/// The regression (#705): Phase 1 sorted the creations parents-first but flushed
/// only after the whole pass, so `tree/a` resolved its parent against a `tree` row
/// still sitting in the writer queue, missed, and escalated to a rescan the settle
/// delay holds back for 30 s. Until then the listing of `tree/a` logged "Parent
/// path not found in index" and showed `<dir>` for every folder size.
#[test]
fn a_tree_created_in_one_batch_lands_every_level() {
    let (writer, db_path, _db_dir) = rename_test_setup();
    let base = rename_test_tempdir();
    let base_abs = IndexPathSpace::root().absolute(&native_text(base.path()));
    insert_path_chain(&db_path, Path::new(&base_abs), &writer);
    std::fs::create_dir_all(base.path().join("tree/a/b")).unwrap();

    let mut reconciler = EventReconciler::new();
    reconciler.switch_to_live();
    // Park the drain, so only the batch itself can have written the rows.
    reconciler.set_rescan_active_for_test(true);

    let mut pending_events: HashMap<String, watcher::FsChangeEvent> = HashMap::new();
    for (id, relative) in ["tree", "tree/a", "tree/a/b"].iter().enumerate() {
        let path = format!("{base_abs}/{relative}");
        pending_events.insert(path.clone(), make_event(&path, id as u64 + 1, created_dir()));
    }
    run_live_batch(&mut pending_events, &mut reconciler, &writer, &db_path);

    let conn = IndexStore::open_write_connection(&db_path).unwrap();
    for relative in ["tree", "tree/a", "tree/a/b"] {
        let path = format!("{base_abs}/{relative}");
        assert!(
            store::resolve_path(&conn, &path).unwrap().is_some(),
            "{relative} should be in the index after the batch"
        );
    }

    writer.shutdown();
}
