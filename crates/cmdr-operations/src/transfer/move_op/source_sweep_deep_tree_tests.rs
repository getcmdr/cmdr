//! A cross-FS move's count of what appeared in a source folder
//! (`count_unscanned_entries`) holds the deepest tree macOS can name on the
//! 2 MiB blocking-pool thread it runs on (#1541):
//! `crate::local_deep_tree_tests` says why these walks may recurse.
#![cfg(unix)] // A path this deep outgrows a Windows path without the long-path prefix.

use std::collections::HashSet;
use std::path::PathBuf;

use super::{SidecarRule, count_unscanned_entries};
use crate::local_deep_tree_tests::{DEPTH, WALK_STACK, deep_tree, on_a_thread};
use cmdr_fs::os_junk_files::OsJunkFiles;

/// Every folder was scanned and the bottom file wasn't, so the count goes down
/// the whole spine and finds that one newcomer.
#[test]
fn counting_newcomers_walks_the_deepest_tree_macos_can_name_on_its_threads_stack() {
    let dir = tempfile::tempdir().unwrap();
    let top = deep_tree(dir.path());
    let mut scanned: HashSet<PathBuf> = HashSet::new();
    let mut folder = top.clone();
    for _ in 0..DEPTH {
        folder = folder.join("d");
        scanned.insert(folder.clone());
    }

    let appeared = on_a_thread(WALK_STACK, move || {
        count_unscanned_entries(&top, &scanned, &SidecarRule::new(), OsJunkFiles::Include)
    });

    assert_eq!(appeared, 1, "the one file nobody scanned");
}
