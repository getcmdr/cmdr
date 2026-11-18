//! What "Go to path" does TODAY with a trailing `/` and a `..` that crosses a
//! symlink. It normalizes lexically, so `x/link/../y` lands on `x/y` by its text,
//! where the OS (a stat, a listing, a delete of the same string) lands beside the
//! link's TARGET (`cmdr-local`'s `wire_path_shape_test.rs`). The volume-aware `..`
//! resolver flips the absolute case on a POSIX disk to the target's sibling; a
//! leading `..` keeps following the pane, as here. #1225.

#![cfg(unix)] // POSIX symlinks.

use super::*;
use crate::test_support::TestDir;

/// `x/link -> ../real/inner`, a folder `x/y` (the lexical answer), and a folder
/// `real/y` (the OS's answer), so each landing names itself.
fn link_tree(name: &str) -> (TestDir, String) {
    let dir = TestDir::new(name);
    std::fs::create_dir_all(dir.join("real/inner")).unwrap();
    std::fs::create_dir_all(dir.join("real/y")).unwrap();
    std::fs::create_dir_all(dir.join("x/y")).unwrap();
    std::os::unix::fs::symlink("../real/inner", dir.join("x/link")).unwrap();
    let root = dir.to_string_lossy().into_owned();
    (dir, root)
}

fn directory(path: String) -> GoToPathResolution {
    GoToPathResolution::Directory { path }
}

#[test]
fn a_trailing_slash_on_a_folder_link_lands_on_the_link_path_without_it() {
    let (_dir, root) = link_tree("go_to_wire_slash");
    assert_eq!(
        resolve(&format!("{root}/x/link/"), "/"),
        directory(format!("{root}/x/link"))
    );
}

#[test]
fn a_dot_dot_after_a_folder_link_is_lexical_today() {
    let (_dir, root) = link_tree("go_to_wire_dot_dot");
    assert_eq!(
        resolve(&format!("{root}/x/link/../y"), "/"),
        directory(format!("{root}/x/y")),
        "today: by the text, ❌ not `real/y` where the OS would land"
    );
}

#[test]
fn a_leading_dot_dot_steps_up_from_the_pane_inside_a_folder_link() {
    let (_dir, root) = link_tree("go_to_wire_leading_dot_dot");
    assert_eq!(
        resolve("../y", &format!("{root}/x/link")),
        directory(format!("{root}/x/y"))
    );
}
