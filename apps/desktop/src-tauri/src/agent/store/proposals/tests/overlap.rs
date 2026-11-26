//! Two pending groups naming one file (#1524): a newer suggestion takes the file over, and a
//! settled run retires every other pending op still naming what it changed.

use rusqlite::Connection;

use super::super::*;
use super::{migrated_conn, status_of};
use crate::agent::types::{OpStatus, ProposalStatus};

/// A trash group over `paths` on `volume`, in a sweep of its own, as a later night's sweep would
/// add it.
fn trash_group(conn: &Connection, volume: &str, paths: &[&str], now: i64) -> i64 {
    let set_id = create_sweep(conn, &NewSweep::default(), now).expect("sweep");
    create_group(conn, set_id, &trash_of(volume, paths), now).expect("group")
}

fn trash_of(volume: &str, paths: &[&str]) -> NewGroup {
    NewGroup {
        intent: GroupIntent::Trash {
            sources: paths
                .iter()
                .map(|path| NewOp {
                    source_path: (*path).to_string(),
                    snapshot: None,
                })
                .collect(),
        },
        source_volume_id: volume.to_string(),
        display_name: "test group".to_string(),
        rationale: None,
        selector: None,
    }
}

fn statuses(conn: &Connection, group_id: i64) -> Vec<(String, OpStatus)> {
    page_ops(conn, group_id, 100, 0)
        .expect("ops")
        .into_iter()
        .map(|op| (op.source_path, op.status))
        .collect()
}

fn revision_of(conn: &Connection, group_id: i64) -> i64 {
    get_group(conn, group_id).expect("read").expect("exists").revision
}

const A: &str = "/test_root/a.jpg";
const B: &str = "/test_root/b.jpg";
const C: &str = "/test_root/c.jpg";

/// Newest wins, per file: the older group keeps its rows (the decision record says what was
/// offered) but the overlapping one leaves its live set, and the group's revision moves so an
/// approval of the version the user read refuses rather than running fewer files unannounced.
#[test]
fn a_newer_group_naming_a_file_supersedes_the_older_pending_op() {
    let conn = migrated_conn();
    let older = trash_group(&conn, "root", &[A, B], 100);

    let newer = trash_group(&conn, "root", &[B, C], 200);

    assert_eq!(
        statuses(&conn, older),
        vec![
            (A.to_string(), OpStatus::Pending),
            (B.to_string(), OpStatus::Superseded)
        ]
    );
    assert_eq!(
        status_of(&conn, older),
        ProposalStatus::Pending,
        "it still has A to offer"
    );
    assert_eq!(
        revision_of(&conn, older),
        1,
        "the older group changed under any open review"
    );
    assert_eq!(
        statuses(&conn, newer),
        vec![(B.to_string(), OpStatus::Pending), (C.to_string(), OpStatus::Pending)],
        "the newer group is whole"
    );
    assert_eq!(count_pending(&conn, ProposalStatus::Pending).expect("count"), (2, 3));
}

/// A group left with nothing to offer retires: nobody answered it, so it is neither rejected
/// nor completed, and it drops off the pending list.
#[test]
fn an_older_group_whose_every_file_a_newer_one_names_retires() {
    let conn = migrated_conn();
    let older = trash_group(&conn, "root", &[A, B], 100);

    trash_group(&conn, "root", &[A, B, C], 200);

    assert_eq!(status_of(&conn, older), ProposalStatus::Retired);
    assert!(
        get_group(&conn, older)
            .expect("read")
            .expect("exists")
            .decided_at
            .is_some()
    );
    assert_eq!(count_pending(&conn, ProposalStatus::Pending).expect("count"), (1, 3));
}

/// An op an earlier review turned off is still the older group's claim on that file, so it is
/// superseded too, and only an op a run already answered keeps its outcome.
#[test]
fn an_excluded_op_is_superseded_too_and_a_settled_group_is_left_alone() {
    let conn = migrated_conn();
    let older = trash_group(&conn, "root", &[A, B], 100);
    record_acceptance(
        &conn,
        older,
        0,
        &OpSelection::all_except(&[page_ops(&conn, older, 10, 0).expect("ops")[1].id]),
        150,
    )
    .expect("preflight");
    let finished = trash_group(&conn, "root", &[C], 110);
    record_acceptance(&conn, finished, 0, &OpSelection::all_except(&[]), 150).expect("preflight");
    claim_group_for_execution(&conn, finished, 150).expect("claim");

    trash_group(&conn, "root", &[B, C], 200);

    assert_eq!(statuses(&conn, older)[1], (B.to_string(), OpStatus::Superseded));
    assert_eq!(
        statuses(&conn, finished),
        vec![(C.to_string(), OpStatus::Pending)],
        "an approved group is the user's answer, never rewritten"
    );
}

/// The same text on another volume is another file.
#[test]
fn the_same_path_on_another_volume_is_another_file() {
    let conn = migrated_conn();
    let older = trash_group(&conn, "smb-testnas", &[A], 100);

    trash_group(&conn, "root", &[A], 200);

    assert_eq!(statuses(&conn, older), vec![(A.to_string(), OpStatus::Pending)]);
    assert_eq!(revision_of(&conn, older), 0);
}

/// An amend is the newest word too: it supersedes the other groups' ops on the files it now names.
#[test]
fn an_amend_supersedes_like_a_new_group() {
    let conn = migrated_conn();
    let older = trash_group(&conn, "root", &[A], 100);
    let amended = trash_group(&conn, "root", &[C], 110);

    let outcome = repropose_group(&conn, amended, &trash_of("root", &[A]), 200).expect("amend");

    assert_eq!(outcome, ReproposeOutcome::Reproposed);
    assert_eq!(status_of(&conn, older), ProposalStatus::Retired);
}

/// A retired op stays out whatever the selection says: neither "all" nor naming it ticks it back.
#[test]
fn no_selection_ticks_a_superseded_op_back_on() {
    let conn = migrated_conn();
    let older = trash_group(&conn, "root", &[A, B], 100);
    trash_group(&conn, "root", &[B], 200);
    let superseded = page_ops(&conn, older, 10, 0).expect("ops")[1].id;

    for selection in [
        OpSelection::all_except(&[]),
        OpSelection::Only {
            op_ids: vec![superseded, page_ops(&conn, older, 10, 0).expect("ops")[0].id],
        },
    ] {
        let outcome = record_acceptance(&conn, older, 1, &selection, 300).expect("preflight");
        assert!(
            matches!(&outcome, AcceptanceOutcome::Accepted { binding } if binding.op_count == 1),
            "{outcome:?}"
        );
    }
    assert_eq!(statuses(&conn, older)[1].1, OpStatus::Superseded);
}

/// A run that MOVED a file away retires every other pending op still naming its old path: the
/// path is no longer the file those ops meant.
#[test]
fn a_settled_run_marks_other_groups_ops_on_its_changed_files_handled() {
    let conn = migrated_conn();
    let other = trash_group(&conn, "root", &[A, B], 100);
    let winner = trash_group(&conn, "root", &[C], 110);
    // Two pending groups can't share a file any more (newest wins), so the overlap a run meets is
    // a file the winner names that an op elsewhere names too, written straight in: the shape a
    // database from before newest-wins holds.
    conn.execute(
        "INSERT INTO proposal_ops (group_id, seq, source_path, status, created_at) VALUES (?1, 1, ?2, 'pending', 110)",
        rusqlite::params![winner, A],
    )
    .expect("overlap");
    record_acceptance(&conn, winner, 0, &OpSelection::all_except(&[]), 150).expect("preflight");
    claim_group_for_execution(&conn, winner, 150).expect("claim");
    for op in page_ops(&conn, winner, 10, 0).expect("ops") {
        let outcome = if op.source_path == A {
            OpStatus::Done
        } else {
            OpStatus::Skipped
        };
        record_op_outcome(&conn, op.id, outcome).expect("outcome");
    }
    mark_group_completed(&conn, winner).expect("settle");

    let retirement = retire_handled_elsewhere(&conn, winner, 300).expect("retire");

    assert_eq!(
        statuses(&conn, other),
        vec![
            (A.to_string(), OpStatus::HandledElsewhere),
            (B.to_string(), OpStatus::Pending)
        ],
        "only the file the run changed is handled; a skipped one is untouched"
    );
    assert_eq!(retirement.shrunk, vec![other]);
    assert!(retirement.retired.is_empty());
    assert_eq!(revision_of(&conn, other), 1);
}

/// A copy leaves its source where it was, so the other groups' ops on it still mean that file.
#[test]
fn a_settled_copy_leaves_other_groups_alone() {
    let conn = migrated_conn();
    let set_id = create_sweep(&conn, &NewSweep::default(), 100).expect("sweep");
    let copy = create_group(
        &conn,
        set_id,
        &NewGroup {
            intent: GroupIntent::Copy {
                destination: WritableDestination::new(Location {
                    volume_id: "root".to_string(),
                    path: "/test_root/backup".to_string(),
                })
                .expect("a folder"),
                sources: vec![TransferOp {
                    source_path: C.to_string(),
                    new_name: None,
                    snapshot: None,
                }],
            },
            source_volume_id: "root".to_string(),
            display_name: "a copy".to_string(),
            rationale: None,
            selector: None,
        },
        100,
    )
    .expect("copy group");
    let other = trash_group(&conn, "root", &[A], 110);
    conn.execute(
        "UPDATE proposal_ops SET source_path = ?2 WHERE group_id = ?1",
        rusqlite::params![copy, A],
    )
    .expect("overlap");
    record_acceptance(&conn, copy, 0, &OpSelection::all_except(&[]), 150).expect("preflight");
    claim_group_for_execution(&conn, copy, 150).expect("claim");
    let op = page_ops(&conn, copy, 10, 0).expect("ops")[0].id;
    record_op_outcome(&conn, op, OpStatus::Done).expect("outcome");

    let retirement = retire_handled_elsewhere(&conn, copy, 300).expect("retire");

    assert_eq!(retirement, Retirement::default());
    assert_eq!(statuses(&conn, other), vec![(A.to_string(), OpStatus::Pending)]);
}
