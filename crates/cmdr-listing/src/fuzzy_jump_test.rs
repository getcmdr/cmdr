//! Fuzzy jump: which row a typed prefix lands on.

use super::fuzzy_jump::*;
use crate::testing::{TestEngine, TestListing};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::staging::StagingTemp;
use std::sync::Arc;

fn entry(name: &str) -> FileEntry {
    FileEntry::new(name.to_string(), format!("/{}", name), false, false)
}

/// The rows a pane shows for a fixture of ordinary names, matching what
/// `CachedListing::rows` produces. The dotfile half only; the scratch half
/// needs a real listing, which `the_jump_lands_on_the_row_the_pane_shows`
/// covers.
fn rows(entries: &[FileEntry], include_hidden: bool) -> impl Iterator<Item = &FileEntry> {
    entries.iter().filter(move |e| include_hidden || !e.is_hidden)
}

#[test]
fn empty_listing_returns_none() {
    let entries: Vec<FileEntry> = Vec::new();
    assert_eq!(find_first_match(rows(&entries, true), "abc"), None);
}

#[test]
fn empty_query_returns_none() {
    let entries = vec![entry("README.md"), entry("AGENTS.md")];
    assert_eq!(find_first_match(rows(&entries, true), ""), None);
}

#[test]
fn no_matches_returns_none() {
    let entries = vec![entry("README.md"), entry("AGENTS.md")];
    // "xyz" shares no characters with either name.
    assert_eq!(find_first_match(rows(&entries, true), "xyz"), None);
}

#[test]
fn a_cyrillic_query_matches_ignoring_case_and_unicode_form() {
    let entries = vec![entry("README.md"), entry("Документы"), entry("Мои\u{306} файл.txt")];
    assert_eq!(find_first_match(rows(&entries, true), "док"), Some(1));
    // A composed "й" finds the name holding it decomposed (и + U+0306).
    assert_eq!(find_first_match(rows(&entries, true), "мой"), Some(2));
}

#[test]
fn single_match_returns_its_index() {
    let entries = vec![entry("README.md"), entry("AGENTS.md"), entry("Cargo.toml")];
    // Only "Cargo.toml" contains the subsequence "crg" / "cargo".
    let idx = find_first_match(rows(&entries, true), "cargo").expect("should match");
    assert_eq!(idx, 2);
}

#[test]
fn multiple_matches_pick_highest_scored() {
    // "tests" fuzzy-matches both, but "tests.js" is the better (prefix) match
    // than "my_tests_helper.rs" so it should win.
    let entries = vec![entry("my_tests_helper.rs"), entry("tests.js"), entry("other.txt")];
    let idx = find_first_match(rows(&entries, true), "tests").expect("should match");
    assert_eq!(idx, 1, "prefix match 'tests.js' should outscore 'my_tests_helper.rs'");
}

#[test]
fn ties_resolve_to_lower_index() {
    // Two identical names → identical scores → lower index wins.
    let entries = vec![entry("hello.txt"), entry("hello.txt")];
    let idx = find_first_match(rows(&entries, true), "hello").expect("should match");
    assert_eq!(idx, 0);
}

#[test]
fn hidden_entry_excluded_when_include_hidden_false() {
    let entries = vec![entry(".env"), entry("env_setup.sh")];
    // With hidden excluded, only "env_setup.sh" is a candidate. The dotfile
    // is invisible, so "env_setup.sh" sits at visible-index 0.
    let idx = find_first_match(rows(&entries, false), "env").expect("should match");
    assert_eq!(idx, 0);
}

#[test]
fn hidden_entry_included_when_include_hidden_true() {
    // Deterministic case: two clearly distinct names. The only entry that can
    // match "alpha" is ".alpha.txt" since "zeta.bin" shares no characters with the
    // query. The match must be found AND must land at the dotfile's visible
    // index (0 when hidden is on, since the dotfile is then visible).
    let entries = vec![entry(".alpha.txt"), entry("zeta.bin")];
    let idx = find_first_match(rows(&entries, true), "alpha").expect("should match");
    assert_eq!(
        idx, 0,
        "hidden '.alpha.txt' must be considered when include_hidden=true"
    );
}

/// Regression test for the visible-space indexing contract.
///
/// Before this fix, `find_first_match` returned the absolute index into the
/// `entries` vec. With a hidden file sitting before the match in the vec,
/// the frontend (which uses the index in the visible sequence, same as
/// `get_file_at` / `get_file_range`) landed one row too far down per
/// skipped dotfile. This test exercises exactly that scenario.
#[test]
fn returns_visible_space_index_when_hidden_precedes_match() {
    // Vec layout: [hidden, hidden, target, other]
    // Absolute indices:   0       1       2       3
    // Visible indices:    -       -       0       1
    let entries = vec![
        entry(".hidden_a"),
        entry(".hidden_b"),
        entry("target.txt"),
        entry("other.bin"),
    ];

    let idx = find_first_match(rows(&entries, false), "target").expect("should match");
    // The visible-space index of "target.txt" is 0, not the absolute 2.
    assert_eq!(
        idx, 0,
        "must return visible-space index (0), not absolute vec index (2), so the frontend cursor doesn't skip rows"
    );

    // Sanity check: with include_hidden=true, the same match lands at
    // visible-index 2 because the two dotfiles are now visible too.
    let idx_with_hidden = find_first_match(rows(&entries, true), "target").expect("should match");
    assert_eq!(idx_with_hidden, 2);
}

#[test]
fn case_insensitive_with_lowercase_query() {
    // Lowercase query → smart case → matches against UPPERCASE filename.
    let entries = vec![entry("README.md"), entry("TESTS.txt"), entry("other.bin")];
    let idx = find_first_match(rows(&entries, true), "tes").expect("should match");
    assert_eq!(idx, 1);
}

#[test]
fn unicode_filename_is_matchable() {
    // Nucleo normalizes Unicode (Normalization::Smart). Typing the ASCII form
    // should still find the accented filename. We document the observed behavior
    // here rather than asserting a strict score. What matters is "some match
    // is found and it's the Résumé entry, not the unrelated one".
    let entries = vec![entry("notes.txt"), entry("Résumé.pdf"), entry("photo.jpg")];
    let idx = find_first_match(rows(&entries, true), "resume").expect("should match");
    assert_eq!(idx, 1, "ASCII 'resume' should fold into 'Résumé.pdf'");
}

/// The contract that matters in the app: the number the jump hands back is a
/// row number in the pane's own space, so the cursor lands on the file the
/// user is looking at.
///
/// Both things the pane leaves out sit before the target here. Deriving the
/// sequence inside this module skipped only the dotfile, so an in-flight
/// scratch file next to a copy in progress moved the cursor one row down.
#[test]
fn the_jump_lands_on_the_row_the_pane_shows() {
    let operation = Arc::new(());
    let temp = StagingTemp::mint(
        std::path::Path::new("/aaa-copying.bin"),
        Some(Arc::downgrade(&operation)),
    );
    let temp_name = temp
        .path()
        .file_name()
        .expect("a minted temp always has a filename")
        .to_string_lossy()
        .into_owned();

    // Rows the pane shows: only "target.txt" and "zzz.bin", at 0 and 1.
    let listing = TestListing::new()
        .volume("test")
        .path("/")
        .entries(vec![
            entry(".hidden"),
            entry(&temp_name),
            entry("target.txt"),
            entry("zzz.bin"),
        ])
        .insert(&TestEngine::new(), "fuzzy-visible-rows");

    assert_eq!(
        listing
            .engine()
            .inner
            .fuzzy_find_first_match_in_listing(listing.id(), "target", false)
            .expect("listing is cached"),
        Some(0),
        "the target is the pane's first row, whatever sits above it in the listing"
    );
}
