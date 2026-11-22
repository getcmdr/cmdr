//! The git chip's commands over the round trip the frontend makes: a pane's app path in, the
//! `repo_root` the info reports back out, and that root handed back to the live channel and the
//! status column.

use cmdr_git::test_fixtures::{Fixture, cleanup, temp_dir};

use super::*;
use crate::file_system::git::EntryStatusCode;

/// The root `get_git_repo_info` reports is native text (`\\?\C:\…` on Windows), and the commands
/// that take it back answer for it: the subscribe hands the info over and the status column
/// sees the modified file. Regression anchor: read as app text, a Windows root was refused.
#[tokio::test]
async fn the_reported_repo_root_is_the_one_subscribe_and_status_take_back() {
    let dir = temp_dir("git_commands", "reported_root");
    let mut fixture = Fixture::init(dir.clone());
    fixture.commit_file("README.md", b"hello\n", "initial");
    std::fs::write(dir.join("README.md"), b"changed\n").expect("modify the file");
    // The pane's path, as a pane holds it: the app path of the folder's long, canonical spelling.
    let canonical = std::fs::canonicalize(&dir).expect("the repo is there");
    let pane = crate::test_support::native_app_path(&canonical).into_string();

    let info = get_git_repo_info(pane.clone())
        .await
        .data
        .expect("the pane's folder is a repo");

    let status = get_git_status_for_paths(info.repo_root.clone(), pane).await.data;
    assert!(
        status
            .iter()
            .any(|entry| entry.relative_path == "README.md" && entry.code == EntryStatusCode::Modified),
        "the status column sees the modified file under the reported root: {status:?}"
    );

    let subscribed = subscribe_git_state(info.repo_root.clone())
        .await
        .expect("the reported root subscribes");
    assert_eq!(subscribed.repo_root, info.repo_root);
    unsubscribe_git_state(info.repo_root).await;

    cleanup(&dir);
}
