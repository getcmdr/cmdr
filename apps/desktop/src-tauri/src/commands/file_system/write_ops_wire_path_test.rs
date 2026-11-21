//! The write starts read their paths at the command edge (`commands::wire_path`): a `..` is
//! refused before anything starts, and a trailing `/` names the entry itself.

#![cfg(unix)] // POSIX symlinks and the OS's own `..` through one.

use super::*;
use crate::test_support::TestDir;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_local::LocalVolume;
use cmdr_operations::testing::CollectorEventSink;

const SETTLE_BUDGET: Duration = Duration::from_secs(10);

/// `x/link -> ../real/inner` with `real/inner/kept.txt`, plus `x/y` and `real/y`: where a
/// lexical `..` and the OS's `..` through the link would each land.
fn link_tree(name: &str) -> TestDir {
    let dir = TestDir::new(name);
    std::fs::create_dir_all(dir.join("real/inner")).unwrap();
    std::fs::write(dir.join("real/inner/kept.txt"), b"kept").unwrap();
    std::fs::write(dir.join("real/y"), b"real").unwrap();
    std::fs::create_dir_all(dir.join("x")).unwrap();
    std::fs::write(dir.join("x/y"), b"x").unwrap();
    std::os::unix::fs::symlink("../real/inner", dir.join("x/link")).unwrap();
    dir
}

fn wire(dir: &TestDir, rel: &str) -> String {
    format!("{}/{rel}", dir.to_string_lossy())
}

/// The boot volume the local starts run against, as production registers it.
fn ensure_root_volume() {
    get_volume_manager().register_if_absent(
        "root",
        Arc::new(LocalVolume::local_folder("Test root", "/", crate::volume_host::host())),
    );
}

async fn settle(events: &CollectorEventSink) {
    crate::test_support::wait_until_async(SETTLE_BUDGET, "the delete settles", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
}

/// The plan's cell: an IPC delete of `x/link/` removes the link, and the target keeps
/// every file: the start drops the `/` (`crates/cmdr-operations/src/wire_paths.rs`), so it
/// names the link itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ipc_delete_of_a_folder_link_with_a_trailing_slash_removes_only_the_link() {
    ensure_root_volume();
    let dir = link_tree("wire_delete_link_slash");
    let events = Arc::new(CollectorEventSink::new());

    delete_files_with(
        Arc::clone(&events) as Arc<dyn OperationEventSink>,
        vec![wire(&dir, "x/link/")],
        None,
        None,
        Initiator::User,
    )
    .await
    .expect("the delete starts");
    settle(&events).await;

    assert!(
        std::fs::symlink_metadata(dir.join("x/link")).is_err(),
        "the link itself is gone"
    );
    assert_eq!(
        std::fs::read(dir.join("real/inner/kept.txt")).expect("the target's file stays"),
        b"kept"
    );
}

/// A `..` reaches no start: refused typed with the generic I/O failure (its copy is
/// "Couldn't delete the file", the path in the technical details), and nothing goes,
/// neither `real/y` (the OS's `..`) nor `x/y` (a lexical one).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ipc_delete_through_a_dot_dot_is_refused_and_deletes_nothing() {
    ensure_root_volume();
    let dir = link_tree("wire_delete_dot_dot");
    for sent in [wire(&dir, "x/link/../y"), "/Volumes/share/../etc".to_string()] {
        let events = Arc::new(CollectorEventSink::new());
        let refused = delete_files_with(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![wire(&dir, "x/y"), sent.clone()],
            None,
            None,
            Initiator::User,
        )
        .await;
        assert!(
            matches!(&refused, Err(WriteOperationError::IoError { path, .. }) if *path == sent),
            "{refused:?}"
        );
        assert!(events.settled.lock_ignore_poison().is_empty(), "nothing started");
    }
    assert!(
        dir.join("real/y").exists() && dir.join("x/y").exists(),
        "nothing deleted"
    );
}

/// A move's sources read the same way: a `..` among them refuses the whole move.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_with_a_dot_dot_source_is_refused() {
    ensure_root_volume();
    let dir = link_tree("wire_move_dot_dot");
    std::fs::create_dir(dir.join("dst")).unwrap();
    let refused = move_files_with(
        Arc::new(CollectorEventSink::new()),
        vec![wire(&dir, "x/link/../y")],
        wire(&dir, "dst"),
        None,
        Initiator::User,
    )
    .await;
    assert!(
        matches!(refused, Err(WriteOperationError::IoError { .. })),
        "{refused:?}"
    );
    assert!(dir.join("real/y").exists() && !dir.join("dst/y").exists());
}

/// The single trash (`move_to_trash`, the rename dialog's "trash the one that's there")
/// reads its path at the edge too: a `..` trashes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_single_trash_through_a_dot_dot_is_refused_and_trashes_nothing() {
    ensure_root_volume();
    let dir = link_tree("wire_trash_single_dot_dot");
    // Over both, so a red run doesn't leave either in the real Trash.
    let _cleanup = cmdr_operations::testing::TrashCleanup::of(&[dir.join("real/y"), dir.join("x/y")]);
    let refused = crate::commands::rename::move_to_trash(wire(&dir, "x/link/../y"), Initiator::User).await;
    assert!(matches!(refused, Err(MutationError::Unexpected { .. })), "{refused:?}");
    assert!(
        dir.join("real/y").exists() && dir.join("x/y").exists(),
        "nothing trashed"
    );
}

/// On macOS and Linux the boot volume's root is `/`, so anchoring a same-root move's
/// destination on it changes nothing: the move lands where it always did.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_same_root_move_lands_in_its_destination_as_before() {
    ensure_root_volume();
    let dir = link_tree("wire_move_dest");
    std::fs::create_dir(dir.join("dst")).unwrap();
    let events = Arc::new(CollectorEventSink::new());
    move_files_with(
        Arc::clone(&events) as Arc<dyn OperationEventSink>,
        vec![wire(&dir, "x/y")],
        wire(&dir, "dst/"),
        None,
        Initiator::User,
    )
    .await
    .expect("the move starts");
    settle(&events).await;
    assert_eq!(std::fs::read(dir.join("dst/y")).expect("moved"), b"x");
}
