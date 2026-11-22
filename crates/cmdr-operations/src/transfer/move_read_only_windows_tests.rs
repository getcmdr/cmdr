//! A local move of a read-only file on Windows (#1331): the read-only attribute on a FILE is
//! Windows' Locked, so a move asks the Locked question before any byte moves, as it asks for
//! Finder's Locked on macOS (`metadata_matrix_tests.rs`' Locked move cells). Windows itself
//! renames a read-only file and `std::fs::remove_file` deletes one, so the question and the
//! sweep's own refusal are all that keep the lock. A folder's read-only bit is Explorer's
//! customization marker, never a lock.
//!
//! A hosted runner has one drive, so the cross-drive cells drive the staging engine
//! (`move_with_staging`) directly, the engine a move between two drives takes.

use std::path::Path;
use std::sync::Arc;

use cmdr_fs::ignore_poison::IgnorePoison;

use crate::testing::TestOperations;
use crate::transfer::move_op::move_files_with_progress_inner;
use crate::transfer::move_op::test_support::{answering_sink, make_state, run_cross_fs_move_with};
use crate::types::{ConflictResolution, LockedKept, LockedSourcesAnswer, WriteOperationConfig, WriteOperationError};

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).expect("the fixture exists").permissions();
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "on Windows this only sets or clears the read-only attribute"
    )]
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).expect("the fixture's attribute changes");
}

fn is_read_only(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .expect("the fixture exists")
        .permissions()
        .readonly()
}

/// `src/frozen.txt` (read-only), `src/plain.txt`, and an empty `dst/`, in one temp folder.
struct Fixture {
    _dir: tempfile::TempDir,
    frozen: std::path::PathBuf,
    plain: std::path::PathBuf,
    dst: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp folder");
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir(&src).expect("create src");
        std::fs::create_dir(&dst).expect("create dst");
        let frozen = src.join("frozen.txt");
        std::fs::write(&frozen, b"keep me").expect("write");
        set_read_only(&frozen, true);
        let plain = src.join("plain.txt");
        std::fs::write(&plain, b"plain").expect("write");
        Self {
            _dir: dir,
            frozen,
            plain,
            dst,
        }
    }

    /// Clears every read-only file left, so the temp folder can go.
    fn release(&self) {
        for path in [&self.frozen, &self.plain] {
            if path.exists() {
                set_read_only(path, false);
            }
        }
        for name in ["frozen.txt", "plain.txt"] {
            let landed = self.dst.join(name);
            if landed.exists() {
                set_read_only(&landed, false);
            }
        }
    }
}

fn same_drive_move(
    fixture: &Fixture,
    answer: LockedSourcesAnswer,
) -> (
    Result<(), WriteOperationError>,
    Arc<crate::event_sinks::CollectorEventSink>,
) {
    let engine = TestOperations::new();
    let state = make_state(&engine, 200);
    let events = answering_sink(&state, answer);
    let result = move_files_with_progress_inner(
        &*events,
        "op-win-same-drive-read-only",
        &state,
        &[fixture.frozen.clone(), fixture.plain.clone()],
        &fixture.dst,
        &WriteOperationConfig::default(),
    );
    (result, events)
}

/// A rename keeps the attribute on Windows, but the move still asks first (macOS's rename
/// refuses a Locked item, so it asks there), and Move anyway moves it, read-only where it lands.
#[test]
fn a_same_drive_move_asks_about_a_read_only_file_and_move_anyway_keeps_it_read_only() {
    let fixture = Fixture::new();

    let (result, events) = same_drive_move(&fixture, LockedSourcesAnswer::MoveAnyway);

    result.expect("the move completes");
    let asked = events.locked_sources.lock_ignore_poison().clone();
    assert_eq!(asked.len(), 1, "asked once");
    assert_eq!(asked[0].locked_count, 1, "about the read-only file alone");
    let landed = fixture.dst.join("frozen.txt");
    assert!(!fixture.frozen.exists() && landed.is_file(), "the read-only file moved");
    assert!(is_read_only(&landed), "and is read-only where it landed");
    assert_eq!(std::fs::read(&landed).expect("read"), b"keep me");
    assert!(fixture.dst.join("plain.txt").is_file());
    fixture.release();
}

#[test]
fn a_same_drive_move_skipping_a_read_only_file_moves_the_rest() {
    let fixture = Fixture::new();

    let (result, events) = same_drive_move(&fixture, LockedSourcesAnswer::Skip);

    result.expect("the move completes");
    assert!(
        fixture.frozen.is_file() && is_read_only(&fixture.frozen),
        "it stays, still read-only"
    );
    assert!(!fixture.dst.join("frozen.txt").exists());
    assert!(
        fixture.dst.join("plain.txt").is_file() && !fixture.plain.exists(),
        "the rest moved"
    );
    let complete = events.complete.lock_ignore_poison().clone();
    assert_eq!(
        complete[0].locked_kept,
        Some(LockedKept {
            skipped: 1,
            system_locked: 0
        })
    );
    fixture.release();
}

/// A folder that's read-only (Explorer's customization marker) moves with no question.
#[test]
fn a_same_drive_move_of_a_read_only_folder_asks_nothing() {
    let fixture = Fixture::new();
    let folder = fixture.plain.parent().expect("src").join("customized");
    std::fs::create_dir(&folder).expect("create the folder");
    set_read_only(&folder, true);
    let engine = TestOperations::new();
    let state = make_state(&engine, 200);
    let events = answering_sink(&state, LockedSourcesAnswer::Skip);

    move_files_with_progress_inner(
        &*events,
        "op-win-same-drive-read-only-folder",
        &state,
        std::slice::from_ref(&folder),
        &fixture.dst,
        &WriteOperationConfig::default(),
    )
    .expect("the move completes");

    assert!(
        events.locked_sources.lock_ignore_poison().is_empty(),
        "nobody was asked"
    );
    let landed = fixture.dst.join("customized");
    assert!(landed.is_dir() && !folder.exists());
    set_read_only(&landed, false);
    fixture.release();
}

/// Asked before anything is copied; on Move anyway the sweep clears the original right
/// before deleting it. The copy lands read-only again, as macOS's lands Locked: the copy reads
/// the attribute off its source as Locked and puts it back after the landing (#1352,
/// `cmdr_fs::metadata`'s Windows arm).
#[test]
fn a_cross_drive_move_asks_before_copying_and_move_anyway_takes_the_read_only_original() {
    let fixture = Fixture::new();
    let engine = TestOperations::new();
    let state = make_state(&engine, 200);
    let events = Arc::new(crate::event_sinks::CollectorEventSink::new());
    let at_the_question = Arc::new(std::sync::Mutex::new(None));
    {
        let (state, dst, seen) = (Arc::clone(&state), fixture.dst.clone(), Arc::clone(&at_the_question));
        events.answer_locked_with(move |_| {
            *seen.lock_ignore_poison() = Some(std::fs::read_dir(&dst).expect("list dst").count());
            state.locked_sources.answer(LockedSourcesAnswer::MoveAnyway);
        });
    }

    run_cross_fs_move_with(
        &state,
        Arc::clone(&events),
        &[fixture.frozen.clone(), fixture.plain.clone()],
        &fixture.dst,
        ConflictResolution::Stop,
        "op-win-cross-drive-read-only",
    )
    .expect("the move completes");

    assert_eq!(
        *at_the_question.lock_ignore_poison(),
        Some(0),
        "asked before anything reached the destination"
    );
    assert_eq!(events.locked_sources.lock_ignore_poison()[0].locked_count, 1);
    assert!(
        !fixture.frozen.exists() && !fixture.plain.exists(),
        "both originals are gone"
    );
    let landed = fixture.dst.join("frozen.txt");
    assert_eq!(std::fs::read(&landed).expect("read"), b"keep me");
    assert!(is_read_only(&landed), "and is read-only again where it landed (#1352)");
    fixture.release();
}

#[test]
fn a_cross_drive_move_skipping_a_read_only_file_copies_none_of_it() {
    let fixture = Fixture::new();
    let engine = TestOperations::new();
    let state = make_state(&engine, 200);

    let events = run_cross_fs_move_with(
        &state,
        answering_sink(&state, LockedSourcesAnswer::Skip),
        &[fixture.frozen.clone(), fixture.plain.clone()],
        &fixture.dst,
        ConflictResolution::Stop,
        "op-win-cross-drive-read-only-skip",
    )
    .expect("the move completes");

    assert!(
        fixture.frozen.is_file() && is_read_only(&fixture.frozen),
        "it stays, still read-only"
    );
    assert!(!fixture.dst.join("frozen.txt").exists(), "and wasn't copied");
    assert!(
        fixture.dst.join("plain.txt").is_file() && !fixture.plain.exists(),
        "the rest moved"
    );
    assert_eq!(events.complete.lock_ignore_poison()[0].files_skipped, 1);
    fixture.release();
}

/// A file that turns read-only after the question had no consent, and `remove_file` would
/// delete it anyway: the sweep refuses it as `FileLocked`, the way macOS's `unlink` refuses a
/// Locked original, and the original stays beside its landed copy.
#[test]
fn a_cross_drive_move_never_deletes_a_read_only_original_nobody_agreed_to() {
    let fixture = Fixture::new();
    let engine = TestOperations::new();
    let state = make_state(&engine, 200);
    let events = Arc::new(crate::event_sinks::CollectorEventSink::new());
    {
        let (state, plain) = (Arc::clone(&state), fixture.plain.clone());
        events.answer_locked_with(move |_| {
            set_read_only(&plain, true);
            state.locked_sources.answer(LockedSourcesAnswer::MoveAnyway);
        });
    }

    let result = run_cross_fs_move_with(
        &state,
        events,
        &[fixture.frozen.clone(), fixture.plain.clone()],
        &fixture.dst,
        ConflictResolution::Stop,
        "op-win-cross-drive-late-read-only",
    );

    assert!(
        matches!(&result, Err(WriteOperationError::FileLocked { path }) if path.ends_with("plain.txt")),
        "{:?}",
        result.err()
    );
    assert_eq!(
        std::fs::read(&fixture.plain).expect("read"),
        b"plain",
        "the original stays"
    );
    assert!(is_read_only(&fixture.plain), "still read-only");
    assert_eq!(
        std::fs::read(fixture.dst.join("plain.txt")).expect("read"),
        b"plain",
        "beside its copy"
    );
    assert!(!fixture.frozen.exists(), "the one agreed to went");
    fixture.release();
}
