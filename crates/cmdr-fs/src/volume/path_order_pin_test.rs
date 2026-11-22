//! How `PathBuf` orders and dedups app paths today, the semantics every `BTreeMap`,
//! `BTreeSet`, `HashSet`, and `sort()` keyed on a path relies on (the index reconciler,
//! the browser build's folder sizes, the in-flight records ledger's derived `Ord`, and
//! about 230 hash sets). The typed app path's `Eq`, `Hash`, and `Ord` are defined to
//! equal these, so a conversion that keys on it keeps every order and every dedup.
//!
//! Component-wise, ❌ never bytewise: the root sorts below any name, then names compare
//! as bytes one at a time, so `/a/b` sorts before `/a b` (as strings, space sorts below
//! `/`), and a trailing `/`, a `.` name, or a doubled `/` disappears.
//!
//! Unix only: on Windows `std::path` compares by Windows rules, the bug #1225 removes.

// Pins `std::path`'s POSIX rules (Windows' reads `//a` as a UNC prefix); `app_path/tests.rs` pins the typed path everywhere.
#![cfg(unix)]

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

const PATHS: [&str; 9] = ["/a/b", "/a-b", "/a.b", "/a b", "/a", "/a/b/c", "/é", "/z", "sftp://h/x"];

#[test]
fn a_path_set_iterates_component_wise() {
    let set: BTreeSet<PathBuf> = PATHS.iter().map(PathBuf::from).collect();
    let order: Vec<&str> = set.iter().map(|p| p.to_str().expect("UTF-8")).collect();
    assert_eq!(
        order,
        ["/a", "/a/b", "/a/b/c", "/a b", "/a-b", "/a.b", "/z", "/é", "sftp://h/x"],
        "a deeper path follows its ancestor before any sibling name; non-ASCII sorts by its bytes; a scheme path sorts after every rooted one"
    );
}

/// The contrast, so a conversion that keys on the TEXT instead is caught: strings sort
/// bytewise, and space, `-`, and `.` all sort below `/`.
#[test]
fn the_same_paths_as_strings_sort_bytewise() {
    let set: BTreeSet<&str> = PATHS.into_iter().collect();
    let order: Vec<&str> = set.into_iter().collect();
    assert_eq!(
        order,
        ["/a", "/a b", "/a-b", "/a.b", "/a/b", "/a/b/c", "/z", "/é", "sftp://h/x"]
    );
}

#[test]
fn a_path_set_holds_every_spelling_of_one_path_once() {
    let set: HashSet<PathBuf> = ["/a", "/a/", "/a//", "/./a", "//a", "/a/."]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(set.len(), 1, "one path, six spellings: {set:?}");

    let kept: HashSet<PathBuf> = ["/a/..", "/a/b/..", "/A", "/a b"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(kept.len(), 4, "`..` is a name, and case and spaces are exact: {kept:?}");
}

/// Equality and order agree on those spellings: `/a/` neither sorts before nor after `/a`.
#[test]
fn a_trailing_slash_compares_equal_and_sorts_equal() {
    let plain = PathBuf::from("/a");
    for spelling in ["/a/", "/a//", "/a/.", "/./a"] {
        let other = PathBuf::from(spelling);
        assert_eq!(plain, other, "{spelling:?}");
        assert_eq!(plain.cmp(&other), std::cmp::Ordering::Equal, "{spelling:?}");
    }
    // ❗ Today's answer, pinned as such: a scheme's `//` collapses like any other doubled
    // `/`, so `std::path` can't tell `sftp://h/x` from `sftp:/h/x`. The typed app path
    // keeps the scheme root whole.
    assert_eq!(PathBuf::from("sftp://h/x"), PathBuf::from("sftp:/h/x/"));
}
