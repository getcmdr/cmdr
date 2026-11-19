//! Only one claim per engine reads, truncates, or sweeps a ledger file, and an
//! engine that never claims leaves every ledger file alone: the type-level
//! stand-in for the one process-wide ledger a second engine could otherwise
//! replay mid-copy.

use std::path::Path;
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;

use super::super::host::OperationErrorReport;
use super::super::testing::TestOperations;
use super::{AlreadyClaimed, Ledger, STORE_FILENAME, TempHome, register};
use cmdr_fs::testing::TestDir;

/// A previous run's ledger in `data_dir` holding one live temp at `temp`, the
/// file still on disk: what a crash mid-copy leaves for the next launch.
fn crashed_run_with_a_live_temp(data_dir: &Path, temp: &Path) -> Vec<u8> {
    std::fs::write(temp, b"partial bytes").expect("the temp is on disk");
    let previous_run = Ledger::recording_in(data_dir);
    let state = TestOperations::new().new_operation_state(Duration::from_millis(50));
    let state = state.with_in_flight_ledger(previous_run.clone());
    register(&state, temp, Some(TempHome::LocalFs));
    previous_run.die();
    std::fs::read(data_dir.join(STORE_FILENAME)).expect("the ledger file is on disk")
}

#[test]
fn a_second_claim_on_one_engine_is_refused_and_leaves_the_ledger_file_as_it_is() {
    let dir = TestDir::new("ledger-second-claim");
    let engine = TestOperations::new();
    engine
        .claim_ledger(Some(&*dir))
        .expect("the first claim is the engine's")
        .wait();
    let temp = dir.join("photo.jpg.cmdr-tmp-11111111-1111-1111-1111-111111111111");
    let state = engine.new_operation_state(Duration::from_millis(50));
    register(&state, &temp, Some(TempHome::LocalFs));
    engine.flush_in_flight_temps();
    let after_first = std::fs::read(dir.join(STORE_FILENAME)).expect("the claimed ledger file");

    assert_eq!(engine.claim_ledger(Some(&*dir)).err(), Some(AlreadyClaimed));
    assert_eq!(
        std::fs::read(dir.join(STORE_FILENAME)).expect("the ledger file"),
        after_first,
        "a refused claim neither truncates nor replays the live ledger"
    );
}

/// The claim's open empties the log, so a record it swept is never replayed
/// again. Regression anchor: Windows refused the old truncate on the append
/// handle and dropped the error, so every launch replayed every record the log
/// ever held, and the log only grew (`log_writer::open_emptied`).
#[test]
fn a_launch_empties_the_ledger_so_the_next_launch_replays_nothing() {
    let dir = TestDir::new("ledger-launch-empties");
    let temp = dir.join("song.mp3.cmdr-tmp-44444444-4444-4444-4444-444444444444");
    let crashed = crashed_run_with_a_live_temp(&dir, &temp);
    assert_eq!(Ledger::replay_log(&dir).len(), 1, "the crashed run left one record");

    let first = TestOperations::new();
    first.claim_ledger(Some(&*dir)).expect("the first launch claims").wait();
    first.flush_in_flight_temps();
    assert!(!temp.exists(), "the first launch swept the temp");
    drop(first);

    assert!(
        Ledger::replay_log(&dir).is_empty(),
        "the second launch replays nothing: the first emptied the log and re-recorded nothing"
    );
    // A retire line alone replays as nothing either way, so the bytes are what
    // tell an emptied log from one that kept growing: the crashed run's record
    // line must be gone, with only the sweep's own retire after it.
    let after = std::fs::read(dir.join(STORE_FILENAME)).expect("the ledger file");
    assert!(
        !after.windows(crashed.len()).any(|line| line == crashed.as_slice()),
        "the first launch emptied the log before writing to it, got {:?}",
        String::from_utf8_lossy(&after)
    );
    let second = TestOperations::new();
    second
        .claim_ledger(Some(&*dir))
        .expect("the second launch claims")
        .wait();
    second.flush_in_flight_temps();
    assert!(Ledger::replay_log(&dir).is_empty(), "and stays empty");
}

#[test]
fn an_engine_that_never_claims_leaves_another_runs_ledger_and_its_temps_alone() {
    let dir = TestDir::new("ledger-unclaimed-engine");
    let temp = dir.join("clip.mov.cmdr-tmp-22222222-2222-2222-2222-222222222222");
    let before = crashed_run_with_a_live_temp(&dir, &temp);

    // A second engine (a test's, a duplicate that lost the install) runs and
    // records, but never claims.
    let unclaimed = TestOperations::new();
    let state = unclaimed.new_operation_state(Duration::from_millis(50));
    let own_temp = dir.join("other.bin.cmdr-tmp-33333333-3333-3333-3333-333333333333");
    register(&state, &own_temp, Some(TempHome::LocalFs));
    unclaimed.flush_in_flight_temps();
    drop(unclaimed);

    assert_eq!(
        std::fs::read(dir.join(STORE_FILENAME)).expect("the ledger file"),
        before,
        "an unclaimed engine never truncates, appends to, or replays a ledger file"
    );
    assert!(temp.exists(), "nor sweeps the temp another run recorded");
}

#[test]
fn a_claim_with_no_data_dir_runs_unpersisted_and_raises_one_error_report() {
    let engine = TestOperations::new();

    engine.claim_ledger(None).expect("the claim is taken").wait();

    let reports = engine.events().error_reports.lock_ignore_poison();
    assert!(
        matches!(reports.as_slice(), [OperationErrorReport::LedgerUnpersisted]),
        "exactly one report, saying the ledger isn't kept: {reports:?}"
    );
    drop(reports);
    let dir = TestDir::new("ledger-no-data-dir");
    assert_eq!(
        engine.claim_ledger(Some(&*dir)).err(),
        Some(AlreadyClaimed),
        "the claim is spent: no later data dir can replay anything"
    );
    assert!(!dir.join(STORE_FILENAME).exists());
}
