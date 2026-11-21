//! A stored path the typed reading refuses (a `..` name, a NUL, anything else no app path
//! takes) is never acted on: the planner records `UnreadablePath` for that item and
//! reverses the rest (`crates/cmdr-operation-log/DETAILS.md` § "The escape mark"). No
//! operation Cmdr journals writes one; these rows stand for a damaged or hand-edited log.
//! (Relative text can't reach a row at all: the journal interns every folder from a root.)

use std::sync::Arc;

use super::test_support::*;
use cmdr_fs::volume::InMemoryVolume;
use cmdr_operation_log::types::{ItemOutcome, OpKind, RollbackState, SkipReason};
use cmdr_operation_log::writer::JournalItem;

/// A finished copy of two photos from the card to the disk: `/photos/a.jpg` journaled as
/// written, and `b.jpg` journaled as `dest_dir` + `dest_name`, which no app path takes.
async fn copy_with_a_refused_row(dest_dir: &str, dest_name: &str) -> (Rig, Arc<InMemoryVolume>) {
    let rig = Rig::new();
    let card = Arc::new(InMemoryVolume::new("Card"));
    let disk = Arc::new(InMemoryVolume::new("Disk"));
    put(&card, "/DCIM/a.jpg", b"photo").await;
    put(&card, "/DCIM/b.jpg", b"photo").await;
    put(&disk, "/photos/a.jpg", b"photo").await;
    put(&disk, "/photos/b.jpg", b"photo").await;
    rig.register("card", card);
    rig.register("disk", disk.clone());
    let refused = JournalItem {
        dest_dir: Some(dest_dir.to_string()),
        dest_name: Some(dest_name.to_string()),
        ..file_unit(1, "card", "/DCIM/b.jpg", "disk", "/photos/b.jpg", 5)
    };
    rig.seed(
        "op",
        OpKind::Copy,
        "card",
        Some("disk"),
        RollbackState::Rollbackable,
        vec![file_unit(0, "card", "/DCIM/a.jpg", "disk", "/photos/a.jpg", 5), refused],
    );
    (rig, disk)
}

async fn the_refused_row_is_held_back_and_the_rest_reverses(dest_dir: &str, dest_name: &str) {
    let (rig, disk) = copy_with_a_refused_row(dest_dir, dest_name).await;

    let report = rig.rollback("op").await;

    assert_eq!((report.reversed, report.skipped), (1, 1), "{report:?}");
    assert_eq!(report.skips.len(), 1, "{:?}", report.skips);
    assert_eq!(report.skips[0].reason, SkipReason::UnreadablePath);
    assert_eq!(report.skips[0].example_name, dest_name, "named as the row stores it");
    assert_eq!(report.final_state, RollbackState::PartiallyRolledBack);
    assert!(!exists(&disk, "/photos/a.jpg").await, "the readable row reverses");
    assert!(exists(&disk, "/photos/b.jpg").await, "nothing acts on the refused row");
    let items = rig.read_items("op");
    let refused = items.iter().find(|item| item.seq == 1).expect("the refused row");
    assert_eq!(refused.outcome, ItemOutcome::Skipped);
    assert_eq!(refused.rollback_skip_reason, Some(SkipReason::UnreadablePath));

    // "Roll back the rest" meets the same row and holds it back again, never acting.
    let again = rig.rollback("op").await;
    assert_eq!((again.reversed, again.skipped), (0, 1), "{again:?}");
    assert!(exists(&disk, "/photos/b.jpg").await);
}

#[tokio::test]
async fn a_row_holding_a_dot_dot_name_is_never_acted_on() {
    the_refused_row_is_held_back_and_the_rest_reverses("/photos/../photos", "b.jpg").await;
}

#[tokio::test]
async fn a_row_holding_a_nul_is_never_acted_on() {
    the_refused_row_is_held_back_and_the_rest_reverses("/photos", "b\0.jpg").await;
}
