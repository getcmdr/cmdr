//! What "Go to path" does with a trailing `/` and a `..` that crosses a symlink
//! (milestone 1's pins (g), flipped by 10c's typed resolver, #1225). A `..` after a
//! name asks the volume, so on a POSIX disk `x/link/../y` lands beside the link's
//! TARGET, where `ls` (and a stat, a listing, a delete of the same string) lands
//! (`cmdr-local`'s `wire_path_shape_test.rs`). A relative input's leading `..` keeps
//! following the pane by text, as the pane's `..` row does.

#![cfg(unix)] // POSIX symlinks.

use super::tests::go;
use super::*;
use crate::test_support::AppTestDir;

/// `x/link -> ../real/inner`, a folder `x/y` (the lexical answer), and a folder
/// `real/y` (the OS's answer), so each landing names itself.
fn link_tree(name: &str) -> (AppTestDir, String) {
    let dir = AppTestDir::new(name);
    std::fs::create_dir_all(dir.native().join("real/inner")).unwrap();
    std::fs::create_dir_all(dir.native().join("real/y")).unwrap();
    std::fs::create_dir_all(dir.native().join("x/y")).unwrap();
    std::os::unix::fs::symlink("../real/inner", dir.native().join("x/link")).unwrap();
    let root = dir.app_path().as_str().to_string();
    (dir, root)
}

fn directory(path: String, crossed_link: bool) -> GoToPathResolution {
    GoToPathResolution::Directory { path, crossed_link }
}

#[tokio::test]
async fn a_trailing_slash_on_a_folder_link_lands_on_the_link_path_without_it() {
    let (dir, root) = link_tree("go_to_wire_slash");
    assert_eq!(
        go(&format!("{root}/x/link/"), dir.app_path()).await,
        directory(format!("{root}/x/link"), false)
    );
}

#[tokio::test]
async fn a_dot_dot_after_a_folder_link_lands_beside_the_target() {
    let (dir, root) = link_tree("go_to_wire_dot_dot");
    assert_eq!(
        go(&format!("{root}/x/link/../y"), dir.app_path()).await,
        directory(format!("{root}/real/y"), true),
        "where `ls` lands, ❌ not `x/y` by the text"
    );
}

#[tokio::test]
async fn a_leading_dot_dot_steps_up_from_the_pane_inside_a_folder_link() {
    let (_dir, root) = link_tree("go_to_wire_leading_dot_dot");
    let pane = AppPathBuf::from_wire(&format!("{root}/x/link")).unwrap();
    assert_eq!(go("../y", &pane).await, directory(format!("{root}/x/y"), false));
}

#[tokio::test]
async fn a_dot_dot_after_a_name_typed_relative_to_the_pane_asks_the_volume_too() {
    let (_dir, root) = link_tree("go_to_wire_relative_dot_dot");
    let pane = AppPathBuf::from_wire(&format!("{root}/x")).unwrap();
    assert_eq!(go("link/../y", &pane).await, directory(format!("{root}/real/y"), true));
}
