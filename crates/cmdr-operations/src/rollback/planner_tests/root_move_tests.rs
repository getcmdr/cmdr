//! An undo after its volume's root moved (a Windows drive back on another letter): it acts
//! on the disk, not on whatever now sits at the old root, and everything it records names
//! the place it acted on.
#![cfg(unix)] // POSIX-only cells: the rig's `InMemoryVolume` paths are POSIX-spelled.

use std::sync::Arc;

use super::test_support::*;
use cmdr_fs::volume::InMemoryVolume;
use cmdr_fs::volume::app_paths::RootMove;
use cmdr_operation_log::store::{open_read_connection, read_rollback_units_page};
use cmdr_operation_log::types::{OpKind, RollbackState};

#[tokio::test]
async fn an_undo_acts_and_journals_on_the_volumes_live_root() {
    let rig = Rig::new();
    let src = Arc::new(InMemoryVolume::new("Src"));
    let dst = Arc::new(InMemoryVolume::new("Dst"));
    // The copy landed under `/E`; the drive is back under `/F`, and another disk's
    // byte-identical file sits at the old address.
    mkdir(&dst, "/F/tree").await;
    put(&dst, "/F/tree/a.txt", b"aaa").await;
    mkdir(&dst, "/E/tree").await;
    put(&dst, "/E/tree/a.txt", b"aaa").await;
    rig.register("src", src);
    rig.register("dst", dst.clone());
    rig.vm.move_root(
        "dst",
        RootMove {
            live: "/F".to_string(),
            earlier: vec!["/E".to_string()],
        },
    );

    rig.seed(
        "op",
        OpKind::Copy,
        "src",
        Some("dst"),
        RollbackState::Rollbackable,
        vec![
            file_unit(0, "src", "/tree/a.txt", "dst", "/E/tree/a.txt", 3),
            dir_unit(1, "dst", "/E/tree"),
        ],
    );
    rig.put_originals("op").await;

    let report = rig.rollback("op").await;
    assert_eq!(report.final_state, RollbackState::RolledBack);
    assert_eq!(report.skipped, 0);
    assert!(!exists(&dst, "/F/tree/a.txt").await, "the copy on the drive is removed");
    assert!(!exists(&dst, "/F/tree").await, "and the folder it made");
    assert!(
        exists(&dst, "/E/tree/a.txt").await,
        "the other disk's file is untouched"
    );

    // The inverse op's own rows name where it acted, so its undo and the crash reconcile
    // read the drive's paths, not the old letter's.
    let conn = open_read_connection(rig.writer.db_path()).expect("conn");
    let rows = read_rollback_units_page(&conn, "inv-1", i64::MAX, 100).expect("inverse rows");
    assert!(!rows.is_empty(), "the inverse op journals what it reversed");
    for row in &rows {
        let paths = [Some(row.source_path.as_path()), row.dest_path.as_deref()];
        for path in paths.into_iter().flatten() {
            assert!(
                !path.starts_with("/E"),
                "a journaled path still names the old root: {path:?}"
            );
        }
    }
}
