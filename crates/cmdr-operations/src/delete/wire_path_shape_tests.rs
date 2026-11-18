//! What a delete does TODAY with a source path shaped in ways a listing row never
//! is: a trailing `/` on a symlink to a folder, and a `..` that crosses a symlink.
//! The IPC `delete_files` hands the frontend's strings to `delete_files_start` as
//! `PathBuf`s unchanged, so these are the wire's answers, through both engines
//! (the local one for the boot disk, the volume one for any other volume id).
//!
//! The app-path type changes both on purpose (a trailing `/` gets dropped, so the
//! path names the link; a `..` gets resolved by the volume, or refused at the
//! wire), and these cells make that a visible flip of a pinned answer. #1225.
#![cfg(unix)] // POSIX symlinks and the OS's own `..` resolution through one.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::super::OperationEventSink;
use super::super::event_sinks::CollectorEventSink;
use super::super::testing::TestOperations;
use super::super::types::WriteOperationConfig;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::{TestDir, wait_until_async};
use cmdr_fs::volume::Volume;
use cmdr_local::LocalVolume;
use cmdr_operation_log::types::Initiator;

const WAIT: Duration = Duration::from_secs(5);

/// `x/link -> ../real/inner` holding `kept.txt`, plus `x/y` (where a lexical
/// `..` lands) and `real/y` (where the OS's `..` through the link lands).
struct LinkTree {
    dir: TestDir,
}

/// What's left on disk after a delete, plus whether it reported a failure.
#[derive(Debug, PartialEq, Eq)]
struct Left {
    link: bool,
    target_folder: bool,
    target_child: bool,
    lexical_y: bool,
    physical_y: bool,
    errors: usize,
}

impl LinkTree {
    fn new(name: &str) -> Self {
        let dir = TestDir::new(name);
        std::fs::create_dir_all(dir.join("real/inner")).unwrap();
        std::fs::write(dir.join("real/inner/kept.txt"), b"the only copy").unwrap();
        std::fs::write(dir.join("real/y"), b"real").unwrap();
        std::fs::create_dir_all(dir.join("x")).unwrap();
        std::fs::write(dir.join("x/y"), b"x").unwrap();
        cmdr_fs::testing::symlink("../real/inner", dir.join("x/link")).unwrap();
        Self { dir }
    }

    /// The wire spelling of `rel` under the tree, byte for byte.
    fn wire(&self, rel: &str) -> PathBuf {
        PathBuf::from(format!("{}/{rel}", self.dir.to_string_lossy()))
    }

    fn left(&self, errors: usize) -> Left {
        let there = |rel: &str| std::fs::symlink_metadata(self.dir.join(rel)).is_ok();
        Left {
            link: there("x/link"),
            target_folder: there("real/inner"),
            target_child: there("real/inner/kept.txt"),
            lexical_y: there("x/y"),
            physical_y: there("real/y"),
            errors,
        }
    }

    /// Deletes `rel` the way the IPC command does, on the boot disk's engine
    /// (`volume: None`) or through a local volume registered under an id.
    async fn delete(&self, rel: &str, through_a_volume: bool) -> Left {
        let engine = TestOperations::new();
        let volume_id = through_a_volume.then(|| {
            let id = format!(
                "wire-shape-{}-{}",
                self.dir.file_name().unwrap().to_string_lossy(),
                std::process::id()
            );
            let volume =
                LocalVolume::local_folder("Test", self.dir.to_path_buf(), crate::host::testing::pane_order_host());
            engine.volumes().register(&id, Arc::new(volume) as Arc<dyn Volume>);
            id
        });
        let events = Arc::new(CollectorEventSink::new());
        let started = engine
            .delete_files_start(
                Arc::clone(&events) as Arc<dyn OperationEventSink>,
                vec![self.wire(rel)],
                WriteOperationConfig::default(),
                volume_id,
                Initiator::User,
                None,
            )
            .await;
        if started.is_ok() {
            wait_until_async(WAIT, "the delete to settle", || {
                !events.settled.lock_ignore_poison().is_empty()
            })
            .await;
        }
        let errors = events.errors.lock_ignore_poison().len() + usize::from(started.is_err());
        self.left(errors)
    }
}

/// The healthy baseline: the bare link path removes the link and keeps its target.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_of_a_folder_link_removes_the_link_and_keeps_its_target() {
    for through_a_volume in [false, true] {
        let tree = LinkTree::new(&format!("wire_shape_delete_bare_{through_a_volume}"));
        assert_eq!(
            tree.delete("x/link", through_a_volume).await,
            Left {
                link: false,
                target_folder: true,
                target_child: true,
                lexical_y: true,
                physical_y: true,
                errors: 0,
            },
            "through a volume: {through_a_volume}"
        );
    }
}

/// ❗ With a trailing `/` the OS follows the link, so BOTH engines walk the TARGET:
/// they delete everything inside the folder the link points at and leave the link
/// in place. On macOS the closing `rmdir("link/")` removes the target folder too; on
/// Linux it refuses (`ENOTDIR`), so the emptied folder stays, and the op still
/// reports no failure. Data loss through the wire today (the frontend never sends a
/// trailing `/`; a raw IPC caller can). Dropping the trailing `/` at the wire makes
/// this cell read like the bare one above.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_of_a_folder_link_with_a_trailing_slash_walks_the_target_today() {
    for through_a_volume in [false, true] {
        let tree = LinkTree::new(&format!("wire_shape_delete_slash_{through_a_volume}"));
        assert_eq!(
            tree.delete("x/link/", through_a_volume).await,
            Left {
                link: true,
                target_folder: cfg!(target_os = "linux"),
                target_child: false,
                lexical_y: true,
                physical_y: true,
                errors: 0,
            },
            "through a volume: {through_a_volume}"
        );
    }
}

/// The OS reads `link/..` as the target's parent, so `x/link/../y` deletes
/// `real/y`, ❌ not the `x/y` its text suggests. Today's answer; the volume-aware
/// `..` resolver keeps it for a POSIX disk on purpose, and `from_wire` refuses
/// a `..` that reaches it unresolved.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_through_a_dot_dot_across_a_folder_link_lands_beside_the_target_today() {
    for through_a_volume in [false, true] {
        let tree = LinkTree::new(&format!("wire_shape_delete_dot_dot_{through_a_volume}"));
        assert_eq!(
            tree.delete("x/link/../y", through_a_volume).await,
            Left {
                link: true,
                target_folder: true,
                target_child: true,
                lexical_y: true,
                physical_y: false,
                errors: 0,
            },
            "through a volume: {through_a_volume}"
        );
    }
}
