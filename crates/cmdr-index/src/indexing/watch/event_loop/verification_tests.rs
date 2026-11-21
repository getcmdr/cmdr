//! The guard's integration tier: real `verify_affected_dirs_with` over real
//! fixtures and a real writer, with the threshold injected so the "over
//! threshold" case is 5 files rather than 1.14M.
//!
//! Every test asserts BOTH halves: the oversized directory is left alone AND
//! a normal directory in the same batch is still fully diffed. Without the
//! second half the test would pass if `verify_affected_dirs` were replaced
//! with `return` (the no-op-fixture anti-pattern in `docs/testing.md`).
//!
//! Pool installation follows `verifier.rs::tests`: a root `ReadPool` under
//! `READ_POOL_TEST_MUTEX`.
//!
//! A `#[path]` child of `verification.rs`, so `super::` here is `verification`.

use super::*;
use crate::indexing::paths::app_text::{native_name, native_text};
use crate::indexing::read::enrichment::{
    READ_POOL_TEST_MUTEX, ReadPool, install_read_pool as install_pool_for, uninstall_read_pool,
};
use crate::indexing::store::{DirStatsById, EntryRow, ROOT_ID};
use cmdr_fs::ignore_poison::IgnorePoison;
use std::fs;
use std::sync::Arc;

/// Temp dir inside the crate root, not `/tmp/`: on Linux `/tmp/` is an
/// excluded prefix, so `should_exclude` would filter every fixture child out
/// and the "normal dir is still diffed" half would pass vacuously.
fn test_tempdir() -> tempfile::TempDir {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    tempfile::Builder::new()
        .prefix("cmdr-verifyguard-")
        .tempdir_in(base)
        .expect("create temp dir")
}

fn setup_writer() -> (IndexWriter, std::path::PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("test-index.db");
    let _store = IndexStore::open(&db_path).expect("open store");
    let writer = IndexWriter::spawn(&db_path, crate::NoopEventSink::shared()).expect("spawn writer");
    (writer, db_path, dir)
}

fn install_read_pool(db_path: &Path) {
    let pool = Arc::new(ReadPool::new(db_path.to_path_buf()).unwrap());
    install_pool_for(ROOT_VOLUME_ID, pool);
}

fn remove_read_pool() {
    uninstall_read_pool(ROOT_VOLUME_ID);
}

/// A directory this pass couldn't LIST tells us nothing about its children, so
/// none of their rows may be swept.
///
/// The order is the whole bug: every child is probed with `Path::exists()`, which
/// is false for any error at all, and the parent's own `read_dir` failure is only
/// consulted afterwards — so an unreadable or vanished parent reaps its entire
/// listing on the way past. A directory the user really deleted still loses its
/// rows, just from its PARENT's pass, whose listing genuinely stops mentioning it.
#[test]
fn a_parent_that_cannot_be_listed_sweeps_none_of_its_children() {
    let _guard = READ_POOL_TEST_MUTEX.lock_ignore_poison();
    let tree = test_tempdir();
    let gone = tree.path().join("gone");
    fs::create_dir_all(&gone).unwrap();
    write_files(&gone, &["one", "two"]);

    let (writer, db_path, _db_dir) = setup_writer();
    let gone_id = ensure_path_in_db(&db_path, &gone, &writer);
    insert_children_from_disk(&writer, gone_id, &gone);
    writer.flush_blocking().unwrap();
    install_read_pool(&db_path);

    // The directory itself goes, so `read_dir` on it fails and every child path
    // stats as absent for a reason that says nothing about the child.
    fs::remove_dir_all(&gone).unwrap();

    let affected: HashSet<String> = [native_text(&gone).to_string()].into_iter().collect();
    verify_affected_dirs_with(&affected, &writer, 100);
    writer.flush_blocking().unwrap();

    let after = db_children(&db_path, gone_id);
    let names: Vec<&str> = after.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"one") && names.contains(&"two"),
        "an unlistable parent proves nothing about its children, so their rows stay: got {names:?}"
    );
    remove_read_pool();
}

/// The control for the pass above: a directory that still LISTS is still diffed,
/// so a child that really went away is still swept. Without this, refusing every
/// sweep would pass the test above and quietly stop verification converging.
#[test]
fn a_parent_that_lists_still_sweeps_a_child_that_went_away() {
    let _guard = READ_POOL_TEST_MUTEX.lock_ignore_poison();
    let tree = test_tempdir();
    let live = tree.path().join("live");
    fs::create_dir_all(&live).unwrap();
    write_files(&live, &["kept", "removed"]);

    let (writer, db_path, _db_dir) = setup_writer();
    let live_id = ensure_path_in_db(&db_path, &live, &writer);
    insert_children_from_disk(&writer, live_id, &live);
    writer.flush_blocking().unwrap();
    install_read_pool(&db_path);

    fs::remove_file(live.join("removed")).unwrap();

    let affected: HashSet<String> = [native_text(&live).to_string()].into_iter().collect();
    verify_affected_dirs_with(&affected, &writer, 100);
    writer.flush_blocking().unwrap();

    let after = db_children(&db_path, live_id);
    let names: Vec<&str> = after.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"kept"), "the surviving child stays: got {names:?}");
    assert!(
        !names.contains(&"removed"),
        "a real removal under a readable parent is still swept: got {names:?}"
    );
    remove_read_pool();
}

/// Insert the directory chain for `path` and return the deepest dir's id.
fn ensure_path_in_db(db_path: &Path, path: &Path, writer: &IndexWriter) -> i64 {
    let conn = IndexStore::open_write_connection(db_path).unwrap();
    let path_str = native_text(path);
    let mut parent_id = ROOT_ID;
    for component in path_str.split('/').filter(|c| !c.is_empty()) {
        parent_id = match IndexStore::resolve_component(&conn, parent_id, component) {
            Ok(Some(id)) => id,
            _ => IndexStore::insert_entry_v2(&conn, parent_id, component, true, false, None, None, None, None).unwrap(),
        };
    }
    let db_next_id = IndexStore::get_next_id(&conn).unwrap();
    writer.next_id().fetch_max(db_next_id, Ordering::Relaxed);
    parent_id
}

/// Index everything currently on disk under `dir_path` as children of `parent_id`.
fn insert_children_from_disk(writer: &IndexWriter, parent_id: i64, dir_path: &Path) {
    for entry in fs::read_dir(dir_path).unwrap().flatten() {
        let meta = fs::symlink_metadata(entry.path()).unwrap();
        let is_dir = meta.is_dir();
        let is_symlink = meta.is_symlink();
        let snap = metadata::extract_metadata(&meta, is_dir, is_symlink);
        let _ = writer.send(WriteMessage::UpsertEntryV2 {
            parent_id,
            name: native_name(&entry.file_name()).into_owned(),
            is_directory: is_dir,
            is_symlink,
            logical_size: snap.logical_size,
            physical_size: snap.physical_size,
            modified_at: snap.modified_at,
            inode: snap.inode,
            nlink: snap.nlink,
        });
    }
    writer.flush_blocking().unwrap();
}

fn db_children(db_path: &Path, parent_id: i64) -> Vec<EntryRow> {
    let conn = IndexStore::open_read_connection(db_path).unwrap();
    IndexStore::list_children_on(parent_id, &conn).unwrap()
}

fn write_files(dir: &Path, names: &[&str]) {
    for name in names {
        fs::write(dir.join(name), "x").unwrap();
    }
}

/// A batch with one over-threshold directory and one normal one.
///
/// Tooth 1 (DB-side): the oversized directory must produce zero per-child
/// upserts, and the normal one must still be fully diffed.
#[test]
fn an_over_threshold_dir_is_declined_while_a_normal_dir_is_still_diffed() {
    let _pool_guard = READ_POOL_TEST_MUTEX.lock_ignore_poison();
    let root = test_tempdir();
    let huge = root.path().join("huge");
    let normal = root.path().join("normal");
    fs::create_dir(&huge).unwrap();
    fs::create_dir(&normal).unwrap();
    write_files(&huge, &["a", "b", "c", "d", "e"]);
    write_files(&normal, &["p", "q"]);

    let (writer, db_path, _db_dir) = setup_writer();
    let huge_id = ensure_path_in_db(&db_path, &huge, &writer);
    let normal_id = ensure_path_in_db(&db_path, &normal, &writer);
    insert_children_from_disk(&writer, huge_id, &huge);
    insert_children_from_disk(&writer, normal_id, &normal);
    install_read_pool(&db_path);

    // Drift on BOTH sides of both directories, so a working diff has
    // something to find in each.
    fs::write(huge.join("new-in-huge"), "x").unwrap();
    fs::remove_file(huge.join("a")).unwrap();
    fs::write(normal.join("new-in-normal"), "x").unwrap();
    fs::remove_file(normal.join("p")).unwrap();

    let affected: HashSet<String> = [native_text(&huge).to_string(), native_text(&normal).to_string()]
        .into_iter()
        .collect();
    // Threshold 3: `huge` has 5 index children (over), `normal` has 2 (under).
    let result = verify_affected_dirs_with(&affected, &writer, 3);
    writer.flush_blocking().unwrap();

    let huge_after = db_children(&db_path, huge_id);
    let normal_after = db_children(&db_path, normal_id);

    // Declined: not one row changed, in either direction.
    let huge_names: Vec<&str> = huge_after.iter().map(|e| e.name.as_str()).collect();
    assert!(
        !huge_names.contains(&"new-in-huge"),
        "a declined dir must produce zero per-child upserts, got {huge_names:?}"
    );
    assert!(
        huge_names.contains(&"a"),
        "a declined dir's stale rows are knowingly left behind (the documented cost), got {huge_names:?}"
    );

    // Still diffed: the normal dir in the SAME batch is fully reconciled.
    let normal_names: Vec<&str> = normal_after.iter().map(|e| e.name.as_str()).collect();
    assert!(
        normal_names.contains(&"new-in-normal"),
        "the normal dir must still gain its new file, got {normal_names:?}"
    );
    assert!(
        !normal_names.contains(&"p"),
        "the normal dir must still lose its stale row, got {normal_names:?}"
    );
    assert_eq!(result.new_file_count, 1, "exactly the normal dir's new file");
    assert_eq!(result.stale_count, 1, "exactly the normal dir's stale row");

    remove_read_pool();
    writer.shutdown();
}

/// Tooth 2 (disk-side): a directory that is small in the index but huge on
/// disk passes the DB probe, so only the iteration cap stands between it and
/// a full re-insertion.
#[test]
fn a_dir_thats_small_in_the_db_but_huge_on_disk_is_truncated_not_fully_diffed() {
    let _pool_guard = READ_POOL_TEST_MUTEX.lock_ignore_poison();
    let root = test_tempdir();
    let sparse = root.path().join("sparse");
    let normal = root.path().join("normal");
    fs::create_dir(&sparse).unwrap();
    fs::create_dir(&normal).unwrap();
    write_files(&sparse, &["known-1", "known-2"]);
    write_files(&normal, &["p"]);

    let (writer, db_path, _db_dir) = setup_writer();
    let sparse_id = ensure_path_in_db(&db_path, &sparse, &writer);
    let normal_id = ensure_path_in_db(&db_path, &normal, &writer);
    insert_children_from_disk(&writer, sparse_id, &sparse);
    insert_children_from_disk(&writer, normal_id, &normal);
    install_read_pool(&db_path);

    // 20 files appear on disk; the index still holds only the original 2, so
    // tooth 1 (threshold 3) waves this directory through.
    let extra: Vec<String> = (0..20).map(|i| format!("disk-{i:02}")).collect();
    write_files(&sparse, &extra.iter().map(String::as_str).collect::<Vec<_>>());
    fs::write(normal.join("new-in-normal"), "x").unwrap();

    let before = DEBUG_STATS.verify_truncated_dirs.load(Ordering::Relaxed);
    let affected: HashSet<String> = [native_text(&sparse).to_string(), native_text(&normal).to_string()]
        .into_iter()
        .collect();
    verify_affected_dirs_with(&affected, &writer, 3);
    writer.flush_blocking().unwrap();

    let sparse_after = db_children(&db_path, sparse_id);
    // 3 iterations max, and 2 of the 22 disk entries are already known, so at
    // most 3 rows can be added on top of the original 2.
    assert!(
        sparse_after.len() <= 5,
        "the iteration cap must stop the diff early, got {} rows",
        sparse_after.len()
    );
    assert!(
        sparse_after.len() < 22,
        "sanity: the cap has to actually bite, got {} rows",
        sparse_after.len()
    );
    assert!(
        DEBUG_STATS.verify_truncated_dirs.load(Ordering::Relaxed) > before,
        "a truncated dir must be counted on the debug surface"
    );

    // The normal dir in the same batch is untouched by the cap.
    let normal_after = db_children(&db_path, normal_id);
    let normal_names: Vec<&str> = normal_after.iter().map(|e| e.name.as_str()).collect();
    assert!(
        normal_names.contains(&"new-in-normal"),
        "the normal dir must still be fully diffed, got {normal_names:?}"
    );

    remove_read_pool();
    writer.shutdown();
}

/// Regression guard, NOT TDD: this can't go red, because the code never wrote
/// the epoch. It exists so a future "let's mark declined dirs honest-stale"
/// change fails loudly.
///
/// Writing `listed_epoch = 0` for a declined dir would look like honesty and
/// be the opposite: `absorbing_min_epoch` propagates the zero to every
/// ancestor, `recursive_size_complete` is derived as `min_subtree_epoch > 0`,
/// so one declined temp directory would render the whole home folder
/// incomplete and make `expected_totals` return `None` for every copy of `~`.
#[test]
fn a_declined_dir_leaves_its_epoch_and_every_ancestor_epoch_untouched() {
    let _pool_guard = READ_POOL_TEST_MUTEX.lock_ignore_poison();
    let root = test_tempdir();
    let huge = root.path().join("huge");
    fs::create_dir(&huge).unwrap();
    write_files(&huge, &["a", "b", "c", "d", "e"]);

    let (writer, db_path, _db_dir) = setup_writer();
    let huge_id = ensure_path_in_db(&db_path, &huge, &writer);
    insert_children_from_disk(&writer, huge_id, &huge);

    // Stamp the dir and give it plus its ancestor chain a positive coverage
    // epoch, exactly as a scan would.
    let epoch = 7u64;
    let ancestors: Vec<i64> = {
        let conn = IndexStore::open_read_connection(&db_path).unwrap();
        let mut chain = Vec::new();
        let mut id = huge_id;
        while let Ok(Some(parent)) = IndexStore::get_parent_id(&conn, id) {
            chain.push(parent);
            if parent == ROOT_ID {
                break;
            }
            id = parent;
        }
        chain
    };
    let _ = writer.send(WriteMessage::MarkDirsListed {
        ids: std::iter::once(huge_id).chain(ancestors.iter().copied()).collect(),
        epoch,
    });
    writer.flush_blocking().unwrap();
    {
        let conn = IndexStore::open_write_connection(&db_path).unwrap();
        let stats: Vec<DirStatsById> = std::iter::once(huge_id)
            .chain(ancestors.iter().copied())
            .map(|entry_id| DirStatsById {
                entry_id,
                recursive_logical_size: 5,
                recursive_physical_size: 5,
                recursive_file_count: 5,
                recursive_dir_count: 0,
                recursive_has_symlinks: false,
                min_subtree_epoch: epoch,
            })
            .collect();
        IndexStore::upsert_dir_stats_by_id(&conn, &stats).unwrap();
    }
    install_read_pool(&db_path);

    // Drift, so a non-declining verification would definitely write here.
    fs::write(huge.join("new-in-huge"), "x").unwrap();
    fs::remove_file(huge.join("a")).unwrap();

    let affected: HashSet<String> = std::iter::once(native_text(&huge).to_string()).collect();
    verify_affected_dirs_with(&affected, &writer, 3);
    writer.flush_blocking().unwrap();

    let conn = IndexStore::open_read_connection(&db_path).unwrap();
    assert_eq!(
        IndexStore::get_listed_epoch_by_id(&conn, huge_id).unwrap(),
        Some(epoch),
        "a declined dir keeps its listed_epoch; writing 0 would drag every ancestor to incomplete"
    );
    for ancestor in std::iter::once(huge_id).chain(ancestors.iter().copied()) {
        let stats = IndexStore::get_dir_stats_by_id(&conn, ancestor).unwrap();
        assert_eq!(
            stats.map(|s| s.min_subtree_epoch),
            Some(epoch),
            "ancestor {ancestor} must keep min_subtree_epoch = {epoch}"
        );
    }

    remove_read_pool();
    writer.shutdown();
}

/// The census hook the walker and the reconcile walk share must be reachable
/// from the guard's own decline path too — otherwise `verify_declined_dirs`
/// reads zero on the machine that motivated all of this.
#[test]
fn a_declined_dir_is_counted_on_the_debug_surface() {
    let _pool_guard = READ_POOL_TEST_MUTEX.lock_ignore_poison();
    let root = test_tempdir();
    write_files(root.path(), &["a", "b", "c", "d", "e"]);

    let (writer, db_path, _db_dir) = setup_writer();
    let dir_id = ensure_path_in_db(&db_path, root.path(), &writer);
    insert_children_from_disk(&writer, dir_id, root.path());
    install_read_pool(&db_path);

    let before = DEBUG_STATS.verify_declined_dirs.load(Ordering::Relaxed);
    let affected: HashSet<String> = std::iter::once(native_text(root.path()).to_string()).collect();
    verify_affected_dirs_with(&affected, &writer, 3);

    assert!(
        DEBUG_STATS.verify_declined_dirs.load(Ordering::Relaxed) > before,
        "the decline must be visible without shipping logs"
    );

    remove_read_pool();
    writer.shutdown();
}
