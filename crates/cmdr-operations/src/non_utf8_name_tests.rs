//! What the write engines do TODAY with a file whose name isn't UTF-8, on Linux
//! (APFS and HFS+ refuse such a name with `EILSEQ`, so macOS can't make one).
//!
//! Two kinds of pin:
//!
//! - **Guards**: copying, moving, or deleting a FOLDER that holds such a name
//!   takes the child along, because the local engine's walks carry the raw
//!   `OsStr`. A change that sends those walks through a lossy or refusing
//!   spelling would drop the child silently; these go red first.
//! - **Today's wrong behavior** (#1236), marked ❗: the row a listing builds has a
//!   lossy name and path (`caf\u{FFFD}.txt`), and every route that addresses a
//!   file by that row misses it, or acts on a UTF-8 sibling literally named
//!   `caf\u{FFFD}.txt`. Pinned so the lossless-name change flips each on purpose,
//!   in its own commit, rather than by accident.

#![cfg(target_os = "linux")]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::volume::{DEFAULT_VOLUME_ID, InMemoryVolume, Volume, WatchCoverage};
use cmdr_listing::testing::{TestListing, TestListingGuard, WatchCoverageVolume};

use crate::delete::delete_files_with_progress_inner;
use crate::event_sinks::CollectorEventSink;
use crate::host::testing::PaneListings;
use crate::testing::TestOperations;
use crate::testing::bench::BenchEngine;
use crate::transfer::copy::copy_files_with_progress_inner;
use crate::transfer::move_op::move_files_with_progress_inner;
use crate::transfer::volume::copy_volumes_with_progress;
use crate::types::{
    ConflictResolution, VolumeCopyConfig, WriteOperationConfig, WriteOperationError, WriteOperationType,
};

/// The non-UTF-8 name: `café.txt` in Latin-1.
const RAW: &[u8] = b"caf\xe9.txt";
/// What every lossy route spells it as, and the UTF-8 sibling's literal name.
const LOSSY: &str = "caf\u{FFFD}.txt";

fn raw() -> &'static OsStr {
    OsStr::from_bytes(RAW)
}

/// A scratch dir holding `src/d/caf\xe9.txt` ("raw") and an empty `dst/`; with
/// `with_sibling`, `d` also holds the UTF-8 `caf\u{FFFD}.txt` ("sibling").
struct Fixture {
    _work: tempfile::TempDir,
    d: PathBuf,
    dst: PathBuf,
}

fn fixture(with_sibling: bool) -> Fixture {
    let work = tempfile::tempdir().expect("scratch");
    let d = work.path().join("src").join("d");
    fs::create_dir_all(&d).expect("d");
    fs::write(d.join(raw()), b"raw").expect("the non-UTF-8 file");
    if with_sibling {
        fs::write(d.join(LOSSY), b"sibling").expect("the U+FFFD sibling");
    }
    let dst = work.path().join("dst");
    fs::create_dir_all(&dst).expect("dst");
    Fixture { _work: work, d, dst }
}

/// `dir`'s children as raw names with their contents, sorted by name bytes.
fn contents_of(dir: &Path) -> Vec<(OsString, Vec<u8>)> {
    let mut children: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| {
            let entry = entry.expect("a dir entry");
            (entry.file_name(), fs::read(entry.path()).expect("a readable file"))
        })
        .collect();
    children.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    children
}

fn raw_file(contents: &[u8]) -> (OsString, Vec<u8>) {
    (raw().to_os_string(), contents.to_vec())
}

fn lossy_file(contents: &[u8]) -> (OsString, Vec<u8>) {
    (OsString::from(LOSSY), contents.to_vec())
}

// ── Guards: the folder's raw-named child goes along ───────────────────

#[test]
fn a_local_copy_of_a_folder_takes_its_non_utf8_child_along() {
    let f = fixture(false);
    BenchEngine::new()
        .copy_local(std::slice::from_ref(&f.d), &f.dst)
        .expect("the copy");
    assert_eq!(contents_of(&f.dst.join("d")), vec![raw_file(b"raw")]);
    assert_eq!(contents_of(&f.d), vec![raw_file(b"raw")], "the source stays");
}

#[test]
fn a_local_move_of_a_folder_takes_its_non_utf8_child_along() {
    let f = fixture(false);
    let engine = TestOperations::new();
    let op_id = "op-move-non-utf8";
    engine.status().register(op_id, WriteOperationType::Move, vec![]);
    let state = Arc::new(engine.new_operation_state(Duration::ZERO));
    move_files_with_progress_inner(
        &CollectorEventSink::new(),
        op_id,
        &state,
        std::slice::from_ref(&f.d),
        &f.dst,
        &WriteOperationConfig::default(),
    )
    .expect("the move");
    engine.status().unregister(op_id);
    assert_eq!(contents_of(&f.dst.join("d")), vec![raw_file(b"raw")]);
    assert!(!f.d.exists(), "the source folder is gone");
}

#[test]
fn a_local_delete_of_a_folder_removes_its_non_utf8_child() {
    let f = fixture(false);
    BenchEngine::new()
        .delete_local(std::slice::from_ref(&f.d))
        .expect("the delete");
    assert!(!f.d.exists(), "the folder and its child are gone");
}

// ── ❗ The listing oracle hands the walk the lossy row path ───────────

/// An engine whose panes show `d` on the local volume, the way an open pane
/// does: the walk takes `d`'s children from the listing (`walk_cached_entries`)
/// instead of reading the disk. The rows come from the real local lister.
fn engine_with_a_pane_on(d: &Path) -> (TestOperations, TestListingGuard) {
    let panes = PaneListings::new();
    panes.show_volume(
        DEFAULT_VOLUME_ID,
        Arc::new(WatchCoverageVolume::new(DEFAULT_VOLUME_ID, WatchCoverage::EveryWriter)) as Arc<dyn Volume>,
    );
    let rows = tokio::runtime::Runtime::new()
        .expect("a runtime for the listing")
        .block_on(BenchEngine::new().local_volume("/").list_directory(d, None))
        .expect("the pane's listing");
    let guard = TestListing::new()
        .volume(DEFAULT_VOLUME_ID)
        .path(d)
        .entries(rows)
        .insert(panes.panes(), "non-utf8-pane");
    (TestOperations::seeing(&panes), guard)
}

fn renaming() -> WriteOperationConfig {
    WriteOperationConfig {
        conflict_resolution: ConflictResolution::Rename,
        ..WriteOperationConfig::default()
    }
}

fn copy_with(engine: &TestOperations, d: &Path, dst: &Path) -> Result<(), WriteOperationError> {
    let state = Arc::new(engine.new_operation_state(Duration::ZERO));
    copy_files_with_progress_inner(
        &CollectorEventSink::new(),
        "op-copy-pane-open",
        &state,
        &[d.to_path_buf()],
        dst,
        &renaming(),
    )
}

/// ❗ Today's wrong behavior (#1236): with `d` open in a pane, the copy scan
/// records the child under its lossy row path, which names no file, so the copy
/// fails where the same copy with no pane open succeeds. A lossless listing row
/// turns this into the guard above.
#[test]
fn with_the_folder_open_in_a_pane_a_local_copy_looks_for_the_lossy_name() {
    let f = fixture(false);
    let (engine, _listing) = engine_with_a_pane_on(&f.d);
    let outcome = copy_with(&engine, &f.d, &f.dst);
    let lossy_path = f.d.join(LOSSY).to_string_lossy().into_owned();
    assert!(
        matches!(&outcome, Err(WriteOperationError::SourceNotFound { path }) if *path == lossy_path),
        "the scan handed the copy the lossy row path: {outcome:?}"
    );
}

/// ❗ Today's wrong behavior (#1236): with the U+FFFD sibling beside it and `d`
/// open in a pane, both rows carry the sibling's path, so the copy lands the
/// SIBLING twice (the second under a renamed name) and never the non-UTF-8 file.
#[test]
fn with_the_folder_open_in_a_pane_a_local_copy_copies_the_lossy_sibling_instead() {
    let f = fixture(true);
    let (engine, _listing) = engine_with_a_pane_on(&f.d);
    copy_with(&engine, &f.d, &f.dst).expect("the copy reports success");
    assert_eq!(
        contents_of(&f.dst.join("d")),
        vec![
            (OsString::from("caf\u{FFFD} (1).txt"), b"sibling".to_vec()),
            lossy_file(b"sibling"),
        ],
        "the sibling landed twice, the non-UTF-8 file never"
    );
}

/// ❗ Today's wrong behavior (#1236): with `d` open in a pane, a delete of `d`
/// reports success and leaves `d` AND its non-UTF-8 child on disk. The delete
/// walk takes the lossy row path from the listing, its removal finds nothing
/// there (already gone, so not an error), and the folder's own removal then
/// meets a folder that isn't empty.
#[test]
fn with_the_folder_open_in_a_pane_a_local_delete_reports_success_and_deletes_nothing() {
    let f = fixture(false);
    let (engine, _listing) = engine_with_a_pane_on(&f.d);
    let state = Arc::new(engine.new_operation_state(Duration::ZERO));
    delete_files_with_progress_inner(
        &CollectorEventSink::new(),
        "op-delete-pane-open",
        &state,
        std::slice::from_ref(&f.d),
        &WriteOperationConfig::default(),
    )
    .expect("the delete reports success");
    assert_eq!(
        contents_of(&f.d),
        vec![raw_file(b"raw")],
        "the folder and its child stay"
    );
}

// ── ❗ The volume engine: local disk to an in-memory volume ───────────

async fn copy_to_memory(f: &Fixture) -> (Result<(), WriteOperationError>, Vec<(String, String, Option<u64>)>) {
    let engine = TestOperations::new();
    let dest = Arc::new(InMemoryVolume::new("mem"));
    dest.create_directory(Path::new("/dst")).await.expect("/dst");
    let state = Arc::new(engine.new_operation_state(Duration::ZERO));
    let outcome = copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-volume-copy",
        &state,
        BenchEngine::new().local_volume("/"),
        std::slice::from_ref(&f.d),
        Arc::clone(&dest) as Arc<dyn Volume>,
        Path::new("/dst"),
        &VolumeCopyConfig {
            conflict_resolution: ConflictResolution::Rename,
            ..VolumeCopyConfig::default()
        },
    )
    .await
    .map_err(|failure| failure.error);
    let mut landed: Vec<_> = dest
        .list_directory(Path::new("/dst/d"), None)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|entry| (entry.name, entry.path, entry.size))
        .collect();
    landed.sort();
    (outcome, landed)
}

/// ❗ Today's wrong behavior (#1236): the volume engine's scan of a local folder
/// hands the copy the child's lossy path, so a copy of `d` onto another volume
/// fails on its only file, after creating the destination folder.
#[tokio::test(flavor = "multi_thread")]
async fn a_volume_copy_of_the_folder_looks_for_the_lossy_name() {
    let f = fixture(false);
    let (outcome, landed) = copy_to_memory(&f).await;
    let lossy_path = f.d.join(LOSSY).to_string_lossy().into_owned();
    assert!(
        matches!(&outcome, Err(WriteOperationError::SourceNotFound { path }) if *path == lossy_path),
        "the scan handed the copy the lossy path: {outcome:?}"
    );
    assert_eq!(landed, vec![], "nothing landed in the destination folder");
}

/// ❗ Today's wrong behavior (#1236): with the U+FFFD sibling beside it, the
/// volume copy of `d` lands the sibling twice (the second renamed) and the
/// non-UTF-8 file never: both rows carry the sibling's path.
#[tokio::test(flavor = "multi_thread")]
async fn a_volume_copy_of_the_folder_copies_the_lossy_sibling_instead() {
    let f = fixture(true);
    let (outcome, landed) = copy_to_memory(&f).await;
    outcome.expect("the copy reports success");
    let sibling_size = Some(b"sibling".len() as u64);
    assert_eq!(
        landed,
        vec![
            (
                "caf\u{FFFD} (1).txt".to_string(),
                "/dst/d/caf\u{FFFD} (1).txt".to_string(),
                sibling_size
            ),
            (LOSSY.to_string(), format!("/dst/d/{LOSSY}"), sibling_size),
        ]
    );
}

// ── ❗ An op on the row path itself ────────────────────────────────────

/// ❗ Today's wrong behavior (#1236): the frontend deletes a row by its path,
/// and the row of a non-UTF-8 name carries the lossy path. With nothing at that
/// path, the delete misses the file.
#[test]
fn a_delete_of_the_lossy_row_path_misses_the_file() {
    let f = fixture(false);
    let outcome = BenchEngine::new().delete_local(&[f.d.join(LOSSY)]);
    let lossy_path = f.d.join(LOSSY).display().to_string();
    assert!(
        matches!(&outcome, Err(WriteOperationError::IoError { path, .. }) if *path == lossy_path),
        "the delete looked for the lossy path: {outcome:?}"
    );
    assert_eq!(contents_of(&f.d), vec![raw_file(b"raw")], "the file stays");
}

/// ❗ Today's wrong behavior (#1236): with a UTF-8 sibling literally named
/// `caf\u{FFFD}.txt`, deleting the non-UTF-8 file's row deletes the SIBLING, a
/// file the person never picked, and keeps the one they did.
#[test]
fn a_delete_of_the_lossy_row_path_deletes_the_literal_sibling() {
    let f = fixture(true);
    BenchEngine::new()
        .delete_local(&[f.d.join(LOSSY)])
        .expect("the delete reports success");
    assert_eq!(
        contents_of(&f.d),
        vec![raw_file(b"raw")],
        "only the picked file is left"
    );
}

// ── ❗ The rename editor's check addresses the old file by its lossy name ──

/// What the rename editor hears for renaming the row named `old_name` in `d`
/// to `new_name`: `(valid, has_conflict, is_case_only_rename)`.
async fn rename_check(d: &Path, old_name: &str, new_name: &str) -> (bool, bool, bool) {
    let engine = TestOperations::new();
    engine.register_local_root();
    let result = engine
        .check_rename_validity_impl(
            d.to_string_lossy().into_owned(),
            old_name.to_string(),
            new_name.to_string(),
            DEFAULT_VOLUME_ID.to_string(),
        )
        .await
        .priced(|_| false);
    (result.valid, result.has_conflict, result.is_case_only_rename)
}

/// ❗ Today's wrong behavior (#1236): the editor sends the row's lossy name as
/// the old name, and the check rebuilds the old path from it. With the U+FFFD
/// sibling beside the non-UTF-8 file, that path is the SIBLING's: pressing
/// Enter on the unchanged prefill reads as a case-only rename of the file onto
/// itself, where it is a rename onto another file.
#[tokio::test(flavor = "multi_thread")]
async fn the_rename_check_reads_the_lossy_sibling_as_the_file_itself() {
    let f = fixture(true);
    assert_eq!(
        rename_check(&f.d, LOSSY, LOSSY).await,
        (true, true, true),
        "valid, a conflict, and a case-only rename of the same file"
    );
    assert_eq!(
        rename_check(&f.d, LOSSY, "new.txt").await,
        (true, false, false),
        "a free name checks out, for the sibling the old path names"
    );
}
