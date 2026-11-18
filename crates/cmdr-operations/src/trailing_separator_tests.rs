//! A source path that names a folder symlink with a trailing `/` (or `/.`) acts on
//! the LINK, ❌ never on the folder it points at. POSIX resolves `x/link/` through
//! the link, so before the engine dropped the trailing separators at its starts
//! (`wire_paths.rs`), a raw IPC `delete_files` of `x/link/` emptied the target.
#![cfg(unix)] // POSIX symlinks and their trailing-slash resolution.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::{TestDir, wait_until_async};
use cmdr_fs::volume::Volume;
use cmdr_local::LocalPosixVolume;
use cmdr_operation_log::types::Initiator;

use super::OperationEventSink;
use super::event_sinks::CollectorEventSink;
use super::testing::TestOperations;
use super::types::WriteOperationConfig;

const WAIT: Duration = Duration::from_secs(5);

/// `x/link -> ../real/inner`, the target holding `kept.txt`.
struct LinkTree {
    dir: TestDir,
}

impl LinkTree {
    fn new(name: &str) -> Self {
        let dir = TestDir::new(name);
        std::fs::create_dir_all(dir.join("real/inner")).unwrap();
        std::fs::write(dir.join("real/inner/kept.txt"), b"the only copy").unwrap();
        std::fs::create_dir_all(dir.join("x")).unwrap();
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        cmdr_fs::testing::symlink("../real/inner", dir.join("x/link")).unwrap();
        Self { dir }
    }

    /// The wire spelling of `rel` under the tree, byte for byte.
    fn wire(&self, rel: &str) -> PathBuf {
        PathBuf::from(format!("{}/{rel}", self.dir.to_string_lossy()))
    }

    fn is_link(&self, rel: &str) -> bool {
        std::fs::symlink_metadata(self.dir.join(rel)).is_ok_and(|m| m.file_type().is_symlink())
    }

    fn target_intact(&self) -> bool {
        std::fs::read(self.dir.join("real/inner/kept.txt")).is_ok_and(|bytes| bytes == b"the only copy")
            && !std::fs::symlink_metadata(self.dir.join("real/inner"))
                .unwrap()
                .file_type()
                .is_symlink()
    }
}

async fn settled(events: &CollectorEventSink) {
    wait_until_async(WAIT, "the op to settle", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
}

async fn delete(tree: &LinkTree, rel: &str, through_a_volume: bool) -> usize {
    let engine = TestOperations::new();
    let volume_id = through_a_volume.then(|| {
        let id = format!("trailing-sep-{}", std::process::id());
        let volume =
            LocalPosixVolume::local_folder("Test", tree.dir.to_path_buf(), super::host::testing::pane_order_host());
        engine.volumes().register(&id, Arc::new(volume) as Arc<dyn Volume>);
        id
    });
    let events = Arc::new(CollectorEventSink::new());
    engine
        .delete_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![tree.wire(rel)],
            WriteOperationConfig::default(),
            volume_id,
            Initiator::User,
            None,
        )
        .await
        .expect("the delete starts");
    settled(&events).await;
    events.errors.lock_ignore_poison().len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_of_a_folder_link_with_a_trailing_separator_removes_the_link_and_keeps_its_target() {
    for through_a_volume in [false, true] {
        for rel in ["x/link/", "x/link//", "x/link/."] {
            let tree = LinkTree::new(&format!("trailing_sep_delete_{through_a_volume}"));
            let errors = delete(&tree, rel, through_a_volume).await;
            assert_eq!(errors, 0, "{rel}, through a volume: {through_a_volume}");
            assert!(tree.target_intact(), "{rel}, through a volume: {through_a_volume}");
            assert!(!tree.is_link("x/link"), "{rel}, through a volume: {through_a_volume}");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_of_a_folder_link_with_a_trailing_separator_moves_the_link() {
    let tree = LinkTree::new("trailing_sep_move");
    let engine = TestOperations::new();
    let events = Arc::new(CollectorEventSink::new());
    engine
        .move_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![tree.wire("x/link/")],
            tree.wire("elsewhere/"),
            WriteOperationConfig::default(),
            vec![],
            None,
            Initiator::User,
            None,
            None,
        )
        .await
        .expect("the move starts");
    settled(&events).await;
    assert!(events.errors.lock_ignore_poison().is_empty());
    assert!(!tree.is_link("x/link"));
    assert!(tree.is_link("elsewhere/link"), "the link moved, as a link");
    assert!(
        tree.dir.join("real/inner/kept.txt").exists(),
        "the target stayed where it was"
    );
}

/// The volume engine's move (what the transfer dialog and Ask Cmdr start).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_volume_move_of_a_folder_link_with_a_trailing_separator_moves_the_link() {
    let tree = LinkTree::new("trailing_sep_volume_move");
    let engine = TestOperations::new();
    let id = format!("trailing-sep-move-{}", std::process::id());
    let volume =
        LocalPosixVolume::local_folder("Test", tree.dir.to_path_buf(), super::host::testing::pane_order_host());
    engine.volumes().register(&id, Arc::new(volume) as Arc<dyn Volume>);
    let events = Arc::new(CollectorEventSink::new());
    engine
        .start_volume_move(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            id.clone(),
            vec![tree.wire("x/link/")],
            id,
            tree.wire("elsewhere/").to_string_lossy().into_owned(),
            super::types::VolumeCopyConfig::default(),
            Initiator::User,
            None,
        )
        .await
        .expect("the move starts");
    settled(&events).await;
    assert!(events.errors.lock_ignore_poison().is_empty());
    assert!(!tree.is_link("x/link"));
    assert!(tree.is_link("elsewhere/link"), "the link moved, as a link");
    assert!(tree.target_intact(), "the target stayed where it was");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_of_a_folder_link_with_a_trailing_separator_renames_the_link() {
    let tree = LinkTree::new("trailing_sep_rename");
    let engine = TestOperations::new();
    engine.register_local_root();
    engine
        .rename_managed(
            tree.wire("x/link/"),
            tree.wire("x/renamed/"),
            false,
            "root".into(),
            Initiator::User,
        )
        .await
        .expect("the rename goes ahead");
    assert!(!tree.is_link("x/link"));
    assert!(tree.is_link("x/renamed"), "the link took the new name");
    assert!(tree.target_intact(), "the target kept its name and contents");
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trash_of_a_folder_link_with_a_trailing_separator_trashes_the_link() {
    let tree = LinkTree::new("trailing_sep_trash");
    // Over both, so a red run (the target trashed) doesn't leave it in the real Trash.
    let _cleanup = super::testing::TrashCleanup::of(&[tree.dir.join("x/link"), tree.dir.join("real/inner")]);
    let engine = TestOperations::new();
    let events = Arc::new(CollectorEventSink::new());
    engine
        .trash_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![tree.wire("x/link/")],
            None,
            WriteOperationConfig::default(),
            Initiator::User,
            None,
        )
        .await
        .expect("the trash starts");
    settled(&events).await;
    assert!(!tree.is_link("x/link"), "the link went to the Trash");
    assert!(tree.target_intact(), "the target stayed");
}
