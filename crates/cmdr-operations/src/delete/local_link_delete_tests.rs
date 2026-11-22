//! The LOCAL walker's permanent delete of a folder link removes the link and keeps its
//! target's contents, and never walks into it. The volume walker's twin is
//! `link_delete_tests.rs`.
//!
//! Regression anchor (#1386, Windows 10 22H2, 2026-11-22): a permanent delete of a junction
//! or a folder symlink refused as "Access is denied", because the walker removed it with
//! `remove_file`. A folder link carries `FILE_ATTRIBUTE_DIRECTORY` on the link itself, and only
//! `RemoveDirectoryW` removes it. On macOS and Linux a folder link is a plain symlink, which
//! `unlink` removes; the cells run there too, so the leaf rule holds on every OS.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::super::OperationEventSink;
use super::super::event_sinks::CollectorEventSink;
use super::super::testing::TestOperations;
use super::super::types::WriteOperationConfig;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::{TestDir, wait_until_async};
use cmdr_operation_log::types::Initiator;

const WAIT: Duration = Duration::from_secs(5);

/// `dir/project/notes.txt`, and `dir/photos/keep.jpg`, the folder the links point at, outside
/// every selection.
fn drive(name: &str) -> TestDir {
    let dir = TestDir::new(name);
    std::fs::create_dir_all(dir.join("project")).expect("mkdir project");
    std::fs::write(dir.join("project/notes.txt"), b"notes").expect("write notes");
    std::fs::create_dir_all(dir.join("photos")).expect("mkdir photos");
    std::fs::write(dir.join("photos/keep.jpg"), b"the only copy").expect("write keep");
    dir
}

fn photos_intact(dir: &Path) -> bool {
    std::fs::read(dir.join("photos/keep.jpg")).is_ok_and(|bytes| bytes == b"the only copy")
}

/// Deletes `sources` permanently through the local walker and waits for it to settle,
/// answering the errors it reported.
async fn delete_permanently(sources: Vec<PathBuf>) -> Vec<String> {
    let engine = TestOperations::new();
    let events = Arc::new(CollectorEventSink::new());
    engine
        .delete_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            sources,
            WriteOperationConfig::default(),
            None,
            Initiator::User,
            None,
        )
        .await
        .expect("start delete");
    wait_until_async(WAIT, "the delete to settle", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
    events
        .errors
        .lock_ignore_poison()
        .iter()
        .map(|error| format!("{error:?}"))
        .collect()
}

/// Makes a folder link at `link` pointing at `target`, answering whether this machine could.
type MakeLink = fn(target: &Path, link: &Path) -> bool;

/// The folder-link shapes a delete meets on this OS. A Windows symlink needs Developer Mode or
/// an elevated token (the hosted runner has the latter); a junction needs neither.
fn folder_link_shapes() -> Vec<(&'static str, MakeLink)> {
    #[cfg(windows)]
    {
        vec![("junction", junction as MakeLink), ("folder symlink", folder_symlink)]
    }
    #[cfg(unix)]
    {
        vec![("folder symlink", posix_symlink as MakeLink)]
    }
}

#[cfg(unix)]
fn posix_symlink(target: &Path, link: &Path) -> bool {
    cmdr_fs::testing::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn junction(target: &Path, link: &Path) -> bool {
    mklink("/J", target, link)
}

#[cfg(windows)]
fn folder_symlink(target: &Path, link: &Path) -> bool {
    mklink("/D", target, link)
}

#[cfg(windows)]
fn mklink(kind: &str, target: &Path, link: &Path) -> bool {
    std::process::Command::new("cmd")
        .args(["/C", "mklink", kind])
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|out| out.status.success())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::print_stderr, reason = "a skip says why")]
async fn a_selected_folder_link_deletes_as_the_link_and_keeps_its_target() {
    for (shape, make) in folder_link_shapes() {
        let dir = drive(&format!("local_link_delete_top_{}", shape.replace(' ', "_")));
        let link = dir.join("project/data");
        if !make(&dir.join("photos"), &link) {
            eprintln!("{}couldn't make a {shape} here", cmdr_fs::testing::SKIP_MARKER);
            return;
        }

        let errors = delete_permanently(vec![link.clone()]).await;

        assert!(errors.is_empty(), "{shape}: {errors:?}");
        assert!(std::fs::symlink_metadata(&link).is_err(), "{shape}: the link is gone");
        assert!(photos_intact(&dir), "{shape}: its target's contents stay");
        assert!(dir.join("photos").is_dir(), "{shape}: its target stays");
        assert!(dir.join("project/notes.txt").exists(), "{shape}: its neighbor stays");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::print_stderr, reason = "a skip says why")]
async fn a_folder_holding_a_folder_link_deletes_without_walking_into_it() {
    for (shape, make) in folder_link_shapes() {
        let dir = drive(&format!("local_link_delete_nested_{}", shape.replace(' ', "_")));
        if !make(&dir.join("photos"), &dir.join("project/data")) {
            eprintln!("{}couldn't make a {shape} here", cmdr_fs::testing::SKIP_MARKER);
            return;
        }

        let errors = delete_permanently(vec![dir.join("project")]).await;

        assert!(errors.is_empty(), "{shape}: {errors:?}");
        assert!(!dir.join("project").exists(), "{shape}: the folder goes, link included");
        assert!(
            photos_intact(&dir),
            "{shape}: the link's target is outside the selection"
        );
    }
}
