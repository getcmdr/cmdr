//! A progress event never shows more done than its total (ERR-BGEKC).
use super::*;
use crate::types::{WriteOperationPhase, WriteOperationType, WriteProgressEvent};

fn event(phase: WriteOperationPhase, files: (usize, usize), bytes: (u64, u64)) -> WriteProgressEvent {
    WriteProgressEvent::new(
        "op".to_owned(),
        WriteOperationType::Copy,
        phase,
        None,
        files.0,
        files.1,
        bytes.0,
        bytes.1,
    )
}

#[test]
fn work_within_the_count_passes_through_untouched() {
    let raised = RaisedTotals::default();
    let mut within = event(WriteOperationPhase::Copying, (5, 10), (500, 1_000));
    raised.apply(&mut within);
    assert_eq!((within.files_total, within.bytes_total), (10, 1_000));
    assert!(!within.outgrew_totals);
}

#[test]
fn work_past_the_count_raises_the_total_and_says_so() {
    // The reporter's screen: `3,122 / 1,016 files`, `23 GB / 6.8 GB`.
    let raised = RaisedTotals::default();
    let mut past = event(WriteOperationPhase::Copying, (3_122, 1_016), (23_000, 6_800));
    raised.apply(&mut past);
    assert_eq!((past.files_total, past.bytes_total), (3_122, 23_000));
    assert!(past.outgrew_totals);
}

#[test]
fn one_axis_past_its_count_raises_only_that_axis() {
    let raised = RaisedTotals::default();
    let mut bytes_only = event(WriteOperationPhase::Copying, (4, 10), (1_500, 1_000));
    raised.apply(&mut bytes_only);
    assert_eq!((bytes_only.files_total, bytes_only.bytes_total), (10, 1_500));
    assert!(bytes_only.outgrew_totals);
}

#[test]
fn an_uncounted_total_stays_uncounted() {
    // Zero is "not counted yet", never "nothing to do": raising it would invent a
    // bar pinned at 100%.
    let raised = RaisedTotals::default();
    let mut scanning = event(WriteOperationPhase::Scanning, (40, 0), (4_000, 0));
    raised.apply(&mut scanning);
    assert_eq!((scanning.files_total, scanning.bytes_total), (0, 0));
    assert!(!scanning.outgrew_totals);

    let mut no_size = event(WriteOperationPhase::Copying, (3, 10), (300, 0));
    raised.apply(&mut no_size);
    assert_eq!(no_size.bytes_total, 0);
    assert!(!no_size.outgrew_totals);
}

#[test]
fn a_draining_rollback_keeps_the_highest_total_so_its_bar_drains() {
    // A rollback counts down from what was done at Cancel. Raising each frame to
    // its own count would pin the bar at 100% until it dipped below the scan's
    // total; the high-water for the phase lets it drain from the first frame.
    let raised = RaisedTotals::default();
    let mut first = event(WriteOperationPhase::RollingBack, (3_122, 1_016), (23_000, 6_800));
    raised.apply(&mut first);
    assert_eq!(first.files_total, 3_122);
    let mut later = event(WriteOperationPhase::RollingBack, (2_000, 1_016), (15_000, 6_800));
    raised.apply(&mut later);
    assert_eq!((later.files_total, later.bytes_total), (3_122, 23_000));
    // Removing what was copied isn't the work outgrowing its count.
    assert!(!first.outgrew_totals && !later.outgrew_totals);
}

#[test]
fn a_new_phase_starts_from_its_own_totals() {
    // A move's copy stage and its source sweep count different things.
    let raised = RaisedTotals::default();
    let mut copying = event(WriteOperationPhase::Copying, (30, 10), (3_000, 1_000));
    raised.apply(&mut copying);
    let mut deleting = event(WriteOperationPhase::Deleting, (1, 2), (0, 0));
    raised.apply(&mut deleting);
    assert_eq!(deleting.files_total, 2);
    assert!(!deleting.outgrew_totals);
}
