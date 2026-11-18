//! The aggregate's rules: which bar the queue becomes, per state.

use super::*;
use cmdr_operations::TransferActivity;
use cmdr_operations::testing::{RollbackSupport, TransferWaitReason};

fn row(id: &str, status: LifecycleStatus) -> OperationSnapshot {
    typed_row(id, WriteOperationType::Copy, status)
}

fn typed_row(id: &str, operation_type: WriteOperationType, status: LifecycleStatus) -> OperationSnapshot {
    OperationSnapshot {
        operation_id: id.to_string(),
        operation_type,
        status,
        source: Some("/src/test_dir".to_string()),
        destination: Some("/dst/test_dir".to_string()),
        rollback_support: RollbackSupport::Supported,
        reverses: None,
        error: None,
        can_retry: false,
        items: None,
        conflict_policy: None,
        started_at: None,
    }
}

fn tick(id: &str, phase: WriteOperationPhase, bytes: (u64, u64), files: (usize, usize)) -> WriteProgressEvent {
    WriteProgressEvent {
        operation_id: id.to_string(),
        operation_type: WriteOperationType::Copy,
        phase,
        current_file: None,
        current_dir: None,
        files_done: files.0,
        files_total: files.1,
        bytes_done: bytes.0,
        bytes_total: bytes.1,
        dirs_done: 0,
        bytes_per_second: None,
        files_per_second: None,
        eta_seconds: None,
        bottleneck: None,
        expected_files_total: None,
        expected_bytes_total: None,
        activity: None,
        step: None,
    }
}

fn copying(id: &str, done: u64, total: u64) -> WriteProgressEvent {
    tick(id, WriteOperationPhase::Copying, (done, total), (0, 1))
}

fn waiting_on(mut event: WriteProgressEvent, reason: TransferWaitReason) -> WriteProgressEvent {
    event.activity = Some(TransferActivity {
        in_flight: 0,
        still_for_seconds: 0,
        waiting_on: reason,
        opening_source: false,
        source_inbound_bytes_per_second: None,
        reconnecting_volume: None,
    });
    event
}

/// Step `number` of `total`. The step type isn't on the engine's public list (its fields carry no
/// docs, which `bindings.ts` would print), so it comes off the wire shape.
fn with_step(event: WriteProgressEvent, number: u8, total: u8) -> WriteProgressEvent {
    let mut wire = serde_json::to_value(event).expect("a progress event serializes");
    wire["step"] = serde_json::json!({ "number": number, "total": total });
    serde_json::from_value(wire).expect("the wire shape reads back")
}

fn shown(percent: Option<u8>, tone: Tone) -> ShellProgress {
    ShellProgress::Shown { percent, tone }
}

// --- Membership and clearing ---

#[test]
fn an_empty_queue_shows_nothing() {
    let mut aggregate = Aggregate::default();
    assert_eq!(aggregate.summary(), ShellProgress::Hidden);
    aggregate.note_operations(&[]);
    assert_eq!(aggregate.summary(), ShellProgress::Hidden);
}

#[test]
fn a_new_operation_shows_an_unmeasured_bar_until_it_measures() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Queued)]);
    assert_eq!(aggregate.summary(), shown(None, Tone::Normal));

    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&tick("op-1", WriteOperationPhase::Scanning, (0, 0), (120, 0)));
    assert_eq!(
        aggregate.summary(),
        shown(None, Tone::Normal),
        "a scan measures nothing"
    );

    aggregate.note_progress(&copying("op-1", 250, 1_000));
    assert_eq!(aggregate.summary(), shown(Some(25), Tone::Normal));
}

#[test]
fn completing_the_last_operation_clears_the_bar() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 900, 1_000));
    aggregate.note_completed("op-1");
    assert_eq!(
        aggregate.summary(),
        shown(Some(99), Tone::Normal),
        "still listed: capped below done"
    );
    aggregate.note_operations(&[]);
    assert_eq!(aggregate.summary(), ShellProgress::Hidden);
}

#[test]
fn cancelling_the_last_operation_clears_the_bar() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 400, 1_000));
    aggregate.note_operations(&[]);
    assert_eq!(aggregate.summary(), ShellProgress::Hidden);
}

#[test]
fn instant_operations_never_show_a_bar() {
    let mut aggregate = Aggregate::default();
    for operation_type in [
        WriteOperationType::Rename,
        WriteOperationType::CreateFolder,
        WriteOperationType::CreateFile,
    ] {
        aggregate.note_operations(&[typed_row("op-1", operation_type, LifecycleStatus::Running)]);
        assert_eq!(aggregate.summary(), ShellProgress::Hidden, "{operation_type:?}");
    }
}

#[test]
fn a_tick_for_an_unlisted_operation_is_ignored() {
    // A late tick after the operation left must not bring it back as a ghost.
    let mut aggregate = Aggregate::default();
    aggregate.note_progress(&copying("op-gone", 500, 1_000));
    assert_eq!(aggregate.summary(), ShellProgress::Hidden);

    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-gone", 500, 1_000));
    assert_eq!(aggregate.summary(), shown(None, Tone::Normal));
}

#[test]
fn a_new_session_starts_from_zero() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 1_000, 1_000));
    aggregate.note_completed("op-1");
    aggregate.note_operations(&[]);

    aggregate.note_operations(&[row("op-2", LifecycleStatus::Running)]);
    assert_eq!(
        aggregate.summary(),
        shown(None, Tone::Normal),
        "op-1 belongs to the last session"
    );
}

// --- Aggregating across operations ---

#[test]
fn byte_measured_operations_weigh_by_their_size() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[
        row("big", LifecycleStatus::Running),
        row("small", LifecycleStatus::Running),
    ]);
    aggregate.note_progress(&copying("big", 100, 9_000));
    aggregate.note_progress(&copying("small", 1_000, 1_000));
    // (100 + 1000) / 10000 = 11%, not the (1% + 100%) / 2 an even split would say.
    assert_eq!(aggregate.summary(), shown(Some(11), Tone::Normal));
}

#[test]
fn a_file_measured_operation_makes_every_operation_weigh_the_same() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[
        row("copy", LifecycleStatus::Running),
        row("delete", LifecycleStatus::Running),
    ]);
    aggregate.note_progress(&copying("copy", 0, 9_000));
    aggregate.note_progress(&tick("delete", WriteOperationPhase::Deleting, (0, 0), (50, 100)));
    assert_eq!(aggregate.summary(), shown(Some(25), Tone::Normal));
}

#[test]
fn an_unmeasured_operation_counts_as_average_sized_and_not_started() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[
        row("running", LifecycleStatus::Running),
        row("queued", LifecycleStatus::Queued),
    ]);
    aggregate.note_progress(&copying("running", 800, 1_000));
    // The queued one's work is real: 80% of one of two equal-sized jobs is 40%.
    assert_eq!(aggregate.summary(), shown(Some(40), Tone::Normal));
}

#[test]
fn a_completed_operation_stays_full_while_others_run() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Running), row("b", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("a", 1_000, 1_000));
    aggregate.note_progress(&copying("b", 0, 1_000));
    aggregate.note_completed("a");
    aggregate.note_operations(&[row("b", LifecycleStatus::Running)]);
    assert_eq!(
        aggregate.summary(),
        shown(Some(50), Tone::Normal),
        "the bar doesn't fall back"
    );
}

#[test]
fn a_snapshot_listing_an_operation_as_done_counts_it_complete() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Running), row("b", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("a", 0, 1_000));
    aggregate.note_progress(&copying("b", 0, 1_000));
    aggregate.note_operations(&[row("a", LifecycleStatus::Done), row("b", LifecycleStatus::Running)]);
    aggregate.note_operations(&[row("b", LifecycleStatus::Running)]);
    assert_eq!(aggregate.summary(), shown(Some(50), Tone::Normal));
}

#[test]
fn a_cancelled_operation_stops_weighing_on_the_bar() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Running), row("b", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("a", 100, 1_000));
    aggregate.note_progress(&copying("b", 500, 1_000));
    aggregate.note_operations(&[row("b", LifecycleStatus::Running)]);
    assert_eq!(aggregate.summary(), shown(Some(50), Tone::Normal));
}

#[test]
fn the_bar_never_reads_done_while_work_is_listed() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 1_000, 1_000));
    assert_eq!(aggregate.summary(), shown(Some(99), Tone::Normal));
}

#[test]
fn a_total_revised_down_mid_flight_clamps_at_full() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 1_500, 1_000));
    assert_eq!(aggregate.summary(), shown(Some(99), Tone::Normal));
}

// --- One operation's measure ---

#[test]
fn a_same_volume_move_measures_files_when_no_bytes_move() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&tick("op-1", WriteOperationPhase::Copying, (0, 0), (3, 4)));
    assert_eq!(aggregate.summary(), shown(Some(75), Tone::Normal));
}

#[test]
fn a_two_step_operation_folds_its_steps_into_one_run() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    let compress = tick("op-1", WriteOperationPhase::Compressing, (500, 1_000), (0, 1));
    let compress = with_step(compress, 1, 2);
    aggregate.note_progress(&compress);
    assert_eq!(aggregate.summary(), shown(Some(25), Tone::Normal));

    let finishing = tick("op-1", WriteOperationPhase::FinishingCompression, (0, 0), (0, 0));
    let finishing = with_step(finishing, 1, 2);
    aggregate.note_progress(&finishing);
    assert_eq!(
        aggregate.summary(),
        shown(Some(50), Tone::Normal),
        "finishing has no denominator: step done"
    );

    let upload = tick("op-1", WriteOperationPhase::Transferring, (200, 400), (0, 1));
    let upload = with_step(upload, 2, 2);
    aggregate.note_progress(&upload);
    assert_eq!(aggregate.summary(), shown(Some(75), Tone::Normal));
}

#[test]
fn rolling_back_holds_the_bar_still() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 600, 1_000));
    aggregate.note_progress(&tick("op-1", WriteOperationPhase::RollingBack, (0, 0), (1, 10)));
    assert_eq!(aggregate.summary(), shown(Some(60), Tone::Normal));
}

#[test]
fn a_tick_with_no_totals_keeps_the_last_measure() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 300, 1_000));
    aggregate.note_progress(&tick("op-1", WriteOperationPhase::Flushing, (0, 0), (0, 0)));
    assert_eq!(aggregate.summary(), shown(Some(30), Tone::Normal));
}

// --- Tones ---

#[test]
fn a_paused_operation_turns_the_bar_paused() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 300, 1_000));
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Paused)]);
    assert_eq!(aggregate.summary(), shown(Some(30), Tone::Paused));
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    assert_eq!(aggregate.summary(), shown(Some(30), Tone::Normal));
}

#[test]
fn a_conflict_waiting_on_a_person_turns_the_bar_paused() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&waiting_on(copying("op-1", 300, 1_000), TransferWaitReason::Conflict));
    assert_eq!(aggregate.summary(), shown(Some(30), Tone::Paused));
    aggregate.note_progress(&waiting_on(copying("op-1", 300, 1_000), TransferWaitReason::Moving));
    assert_eq!(aggregate.summary(), shown(Some(30), Tone::Normal));
}

#[test]
fn a_stall_on_a_slow_share_is_not_a_pause() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    for reason in [
        TransferWaitReason::Destination,
        TransferWaitReason::Source,
        TransferWaitReason::Reconnecting,
        TransferWaitReason::Unknown,
    ] {
        aggregate.note_progress(&waiting_on(copying("op-1", 300, 1_000), reason));
        assert_eq!(aggregate.summary(), shown(Some(30), Tone::Normal), "{reason:?}");
    }
}

#[test]
fn one_moving_operation_keeps_the_bar_normal() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Paused), row("b", LifecycleStatus::Running)]);
    assert_eq!(aggregate.summary(), shown(None, Tone::Normal));
}

#[test]
fn queued_work_behind_a_paused_operation_reads_paused() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Paused), row("b", LifecycleStatus::Queued)]);
    assert_eq!(aggregate.summary(), shown(None, Tone::Paused));
}

#[test]
fn a_queue_of_only_queued_work_is_not_paused() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Queued)]);
    assert_eq!(aggregate.summary(), shown(None, Tone::Normal));
}

#[test]
fn a_failure_alone_shows_a_full_error_bar_until_dismissed() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("op-1", 300, 1_000));
    aggregate.note_operations(&[row("op-1", LifecycleStatus::Failed)]);
    assert_eq!(aggregate.summary(), shown(Some(100), Tone::Error));
    aggregate.note_operations(&[]);
    assert_eq!(aggregate.summary(), ShellProgress::Hidden);
}

#[test]
fn running_work_beside_a_failure_wins_and_the_failure_neither_colors_nor_weighs() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Running), row("b", LifecycleStatus::Running)]);
    aggregate.note_progress(&copying("a", 100, 1_000));
    aggregate.note_progress(&copying("b", 500, 1_000));
    aggregate.note_operations(&[row("a", LifecycleStatus::Failed), row("b", LifecycleStatus::Running)]);
    assert_eq!(aggregate.summary(), shown(Some(50), Tone::Normal));
}

#[test]
fn paused_work_beside_a_failure_reads_paused() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Failed), row("b", LifecycleStatus::Paused)]);
    assert_eq!(aggregate.summary(), shown(None, Tone::Paused));
}

#[test]
fn the_failure_turns_the_bar_red_once_the_last_live_work_ends() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[row("a", LifecycleStatus::Failed), row("b", LifecycleStatus::Running)]);
    aggregate.note_completed("b");
    aggregate.note_operations(&[row("a", LifecycleStatus::Failed), row("b", LifecycleStatus::Done)]);
    assert_eq!(aggregate.summary(), shown(Some(100), Tone::Error));
}

#[test]
fn an_instant_operation_that_failed_shows_no_error() {
    let mut aggregate = Aggregate::default();
    aggregate.note_operations(&[typed_row("op-1", WriteOperationType::Rename, LifecycleStatus::Failed)]);
    assert_eq!(aggregate.summary(), ShellProgress::Hidden);
}
