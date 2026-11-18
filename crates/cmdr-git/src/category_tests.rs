//! What each of the six virtual categories lists: branches, tags, commits,
//! stash, worktrees, submodules, and the row set the `.git/` landing page is
//! built from.
//!
//! Standard init+commit fixtures go through [`Fixture`]. The handful of
//! operations gix exposes no public API for (stash creation, worktree add,
//! submodule add) keep a thin [`git_cli`] shell-out (verified on gix 0.87,
//! 2026-09-05).

#![cfg(test)]

use std::path::PathBuf;

use crate::path::{Cat, VirtualGitPath, classify};
use crate::test_fixtures::{
    Fixture, build_repo_with_a_tag_and_a_slashed_branch, build_simple_repo, cleanup, discover_repo, git_cli,
    git_cli_capture, temp_dir,
};
use crate::{log as git_log, stash, submodules, virtual_listing, worktrees};
use cmdr_fs::git_meta::{GitCountKind, GitEntryMeta};

// ── the six rows the landing page is built from ───────────────────────

/// The six category rows the listing overlay contributes to a repo's `.git/`,
/// in their fixed display order. The real `.git/*` entries beside them are the
/// LOCAL volume's, so they aren't this function's business at all.
#[test]
fn list_categories_yields_the_six_rows_in_display_order() {
    let dir = build_repo_with_a_tag_and_a_slashed_branch("category");
    let (handle, root) = discover_repo(&dir).unwrap();
    let names: Vec<String> = virtual_listing::list_categories(&handle, &root)
        .into_iter()
        .map(|e| e.name)
        .collect();

    assert_eq!(
        names,
        ["branches", "tags", "commits", "stash", "worktrees", "submodules"]
    );
    cleanup(&dir);
}

// ── branches ──────────────────────────────────────────────────────────

/// A branch called `feature/foo` is ONE row called `feature/foo`, never a
/// `feature/` folder with a `foo` inside it.
#[test]
fn list_branches_includes_slashed_name() {
    let dir = build_repo_with_a_tag_and_a_slashed_branch("category");
    let (handle, root) = discover_repo(&dir).unwrap();
    let entries = virtual_listing::list_branches(&handle, &root).unwrap();
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"main"));
    assert!(names.contains(&"feature/foo"));
    for entry in &entries {
        assert!(entry.is_directory);
        assert_eq!(entry.icon_id, "git:branch");
    }
    cleanup(&dir);
}

// ── tags ──────────────────────────────────────────────────────────────

#[test]
fn list_tags_yields_v1() {
    let dir = build_repo_with_a_tag_and_a_slashed_branch("category");
    let (handle, root) = discover_repo(&dir).unwrap();
    let entries = virtual_listing::list_tags(&handle, &root).unwrap();
    assert!(entries.iter().any(|e| e.name == "v1.0"));
    cleanup(&dir);
}

// ── commits ───────────────────────────────────────────────────────────

#[test]
fn list_commits_yields_entries_with_short_sha_and_subject() {
    let (dir, _f) = build_simple_repo("category", 3);
    let (handle, root) = discover_repo(&dir).unwrap();
    let entries = git_log::list_commits(&handle, &root, &git_log::WalkControl::default()).unwrap();
    assert_eq!(entries.len(), 3, "fixture has exactly 3 commits");
    for fe in &entries {
        // Display name = "<short-sha> <subject>"
        assert!(fe.name.contains(' '), "expected sha + subject in {}", fe.name);
        assert_eq!(fe.icon_id, "git:commit");
        assert!(fe.is_directory);
        assert!(fe.modified_at.is_some(), "the committer date dates the row");
        // The portal claims no date beyond Modified (`supported_dates`), so a
        // Created or Added sort falls back to it rather than reading a copy.
        assert_eq!((fe.created_at, fe.added_at), (None, None));
    }
    // Newest first ordering: top entry should match `git log -1`.
    let top_subject = String::from_utf8(git_cli_capture(&dir, &["log", "-1", "--format=%s"])).unwrap();
    assert!(entries[0].name.contains(top_subject.trim()));
    cleanup(&dir);
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "app paths go through std::path on Windows: #1225")]
fn commit_tree_browsing_via_short_sha() {
    let (dir, _f) = build_simple_repo("category", 2);
    let (handle, root) = discover_repo(&dir).unwrap();
    let entries = git_log::list_commits(&handle, &root, &git_log::WalkControl::default()).unwrap();
    let top = &entries[0];
    // The on-disk path segment is the bare 7-char short sha.
    let segment = top.path.split('/').next_back().unwrap().to_string();
    assert_eq!(segment.len(), git_log::SHORT_SHA_LEN);

    // Direct path entry: classify `.git/commits/<sha>` and resolve.
    let p = root.join(".git").join("commits").join(&segment).join("README.md");
    let (virt, _, _) = classify(&p).expect("classify commit path");
    match virt {
        VirtualGitPath::RefTree(Cat::Commits, name, sub) => {
            assert_eq!(name, segment);
            assert_eq!(sub, "README.md");
        }
        _ => panic!("expected commits/<sha>/README.md to classify as RefTree"),
    }
    cleanup(&dir);
}

#[test]
fn commits_caps_listing_at_max() {
    // Building 100,001 commits to reach the cap would take far too long, so this
    // pins the constants the walk stops on and the whole-listing count stops at.
    assert_eq!(git_log::MAX_COMMITS, 100_000);
    assert_eq!(crate::GitPortal::COUNT_ALL_LIMIT, 5_000);
}

#[test]
fn the_walk_leaves_every_files_changed_count_for_later() {
    let (dir, _f) = build_simple_repo("category", 5);
    let (handle, root) = discover_repo(&dir).unwrap();

    let entries = git_log::list_commits(&handle, &root, &git_log::WalkControl::default()).unwrap();

    assert_eq!(entries.len(), 5, "every commit is listed");
    for row in &entries {
        assert_eq!(row.size, None, "an uncounted row has no Size to sort by");
        assert_eq!(
            row.git_meta,
            Some(GitEntryMeta::CountPending {
                counted: GitCountKind::FilesChanged
            }),
            "the pane words it as pending and knows to ask for it",
        );
    }
    cleanup(&dir);
}

#[test]
fn the_walk_publishes_how_many_commits_it_has_listed() {
    let (dir, _f) = build_simple_repo("category", 5);
    let (handle, root) = discover_repo(&dir).unwrap();
    let control = git_log::WalkControl::default();

    let entries = git_log::list_commits(&handle, &root, &control).unwrap();

    assert_eq!(control.listed(), entries.len());
    cleanup(&dir);
}

#[test]
fn an_abandoned_walk_stops_instead_of_listing_for_nobody() {
    let (dir, _f) = build_simple_repo("category", 5);
    let (handle, root) = discover_repo(&dir).unwrap();
    let control = git_log::WalkControl::default();
    control.abandon();

    let entries = git_log::list_commits(&handle, &root, &control).unwrap();

    assert!(entries.is_empty(), "nobody is waiting for these rows");
    cleanup(&dir);
}

#[test]
fn a_commit_row_filled_in_later_is_the_walked_row_plus_its_count() {
    let (dir, _f) = build_simple_repo("category", 3);
    let (handle, root) = discover_repo(&dir).unwrap();
    let uncounted = git_log::list_commits(&handle, &root, &git_log::WalkControl::default()).unwrap();

    let paths: Vec<PathBuf> = uncounted.iter().map(|e| PathBuf::from(&e.path)).collect();
    let filled = git_log::commit_rows_with_counts(&handle, &root, &paths);

    assert_eq!(filled.len(), 3);
    for (filled, walked) in filled.iter().zip(&uncounted) {
        assert_eq!(filled.path, walked.path, "same row, so the listing patches it in place");
        assert_eq!(filled.name, walked.name);
        assert_eq!(filled.modified_at, walked.modified_at);
        let Some(GitEntryMeta::Count {
            counted: GitCountKind::FilesChanged,
            n,
        }) = filled.git_meta
        else {
            panic!("{} came back without its count", filled.name);
        };
        assert_eq!(filled.size, Some(n), "Size sorts by the count");
    }
    cleanup(&dir);
}

#[test]
fn filling_in_counts_skips_a_path_that_is_no_commit_row() {
    let (dir, _f) = build_simple_repo("category", 1);
    let (handle, root) = discover_repo(&dir).unwrap();

    let filled = git_log::commit_rows_with_counts(
        &handle,
        &root,
        &[root.join(".git/commits/zzzzzzz"), root.join(".git/branches/main")],
    );

    assert!(filled.is_empty());
    cleanup(&dir);
}

#[test]
fn commit_path_resolves_unreachable_sha() {
    // Cmdr's claim: `.git/commits/<sha>/...` resolves even if the SHA
    // isn't reachable from HEAD. We simulate that by making a commit on
    // a side branch, deleting the branch (the commit object stays in the
    // ODB), and asserting we can still browse it via its SHA.
    let dir = temp_dir("category", "unreachable");
    let mut f = Fixture::init(dir.clone());
    f.commit_file("README.md", b"step 0\n", "commit 0");

    // Create and switch to a side branch, then commit something only
    // there.
    f.create_branch("side");
    f.checkout("side");
    f.commit_file("side.txt", b"hi\n", "side");
    let side_sha = {
        let r = f
            .repo
            .find_reference("refs/heads/side")
            .unwrap()
            .peel_to_id()
            .unwrap()
            .detach();
        r.to_string()
    };
    f.checkout("main");

    // Delete the `side` branch ref. The commit object itself stays in
    // the ODB, so resolve_commit_id can still find it.
    f.repo
        .edit_reference(gix::refs::transaction::RefEdit {
            change: gix::refs::transaction::Change::Delete {
                expected: gix::refs::transaction::PreviousValue::Any,
                log: gix::refs::transaction::RefLog::AndReference,
            },
            name: "refs/heads/side".try_into().unwrap(),
            deref: false,
        })
        .expect("delete side branch");

    // The commit is no longer HEAD-reachable. List_commits returns the
    // single main commit. But classify + resolve_commit_id still find it.
    let (handle, root) = discover_repo(&dir).unwrap();
    let listing = git_log::list_commits(&handle, &root, &git_log::WalkControl::default()).unwrap();
    assert!(!listing.iter().any(|e| e.name.contains("side")));

    // Direct resolve from the orphaned SHA still works.
    let id = git_log::resolve_commit_id(&handle, &side_sha[..7]).expect("resolve unreachable sha");
    assert!(!id.is_null());
    cleanup(&dir);
}

// ── stash ────────────────────────────────────────────────────────────

#[test]
fn list_stashes_returns_three_entries() {
    let (dir, _f) = build_simple_repo("category", 1);
    let (handle, root) = discover_repo(&dir).unwrap();

    // Three round-trips of "modify, stash". `git stash` refuses an
    // empty stash, so we touch a different file each round. Stash
    // creation has no gix-side API in 0.81; CLI is the only path.
    for n in 0..3 {
        std::fs::write(dir.join(format!("stash-file-{}.txt", n)), "x\n").unwrap();
        // git stash needs the file to be tracked or to be told to keep
        // untracked too.
        git_cli(&dir, &["stash", "push", "-u", "-m", &format!("change {}", n)]);
    }

    // The handle is still useful for `resolve_stash_commit` later in the
    // file; suppress the unused warning for this test.
    let _ = &handle;
    let entries = stash::list_stashes(&root).unwrap();
    assert_eq!(entries.len(), 3);
    // Newest-first ordering – git stash list follows reflog, newest at
    // the top, which means stash@{0} is the most recent.
    assert!(entries[0].name.starts_with("stash@{0}"));
    assert!(entries[2].name.starts_with("stash@{2}"));

    // Resolving stash@{n} via gix's underlying object id matches what
    // `git stash show <n>` would expand to.
    let id = stash::resolve_stash_commit(&handle, 0).unwrap();
    let expected = String::from_utf8(git_cli_capture(&dir, &["rev-parse", "stash@{0}"]))
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(id.to_string(), expected);

    cleanup(&dir);
}

// ── worktrees ────────────────────────────────────────────────────────

#[test]
fn list_worktrees_redirects_to_working_dir() {
    let (dir, _f) = build_simple_repo("category", 1);
    // `git worktree add` has no gix-side public API in 0.81; CLI is the
    // only path. We add a sibling worktree at `<dir>-wt1`.
    let wt_path = dir
        .parent()
        .unwrap()
        .join(format!("{}-wt1", dir.file_name().unwrap().to_string_lossy()));
    let _ = std::fs::remove_dir_all(&wt_path);
    git_cli(&dir, &["worktree", "add", "-b", "wt-branch", wt_path.to_str().unwrap()]);

    let (handle, root) = discover_repo(&dir).unwrap();
    let entries = worktrees::list_worktrees(&handle, &root).unwrap();
    assert_eq!(entries.len(), 1);
    let wt = &entries[0];
    assert!(wt.is_directory);
    assert!(wt.redirect_to_path.is_some(), "redirect_to_path must be set");
    let canonical_wt = wt_path.canonicalize().unwrap();
    assert_eq!(
        PathBuf::from(wt.redirect_to_path.as_ref().unwrap())
            .canonicalize()
            .unwrap(),
        canonical_wt
    );

    cleanup(&dir);
    cleanup(&wt_path);
}

// ── submodules ───────────────────────────────────────────────────────

#[test]
fn list_submodules_redirects_to_working_dir() {
    // Outer repo with one commit.
    let (outer, _of) = build_simple_repo("category", 1);
    // Inner repo to add as submodule.
    let (inner, _if) = build_simple_repo("category", 1);
    // `git submodule add` has no gix-side public API in 0.81; CLI is
    // the only path. Use a `file://` URL or path with `file://`.
    let inner_url = format!("file://{}", inner.display());
    git_cli(
        &outer,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &inner_url,
            "vendor/inner",
        ],
    );
    git_cli(&outer, &["commit", "-q", "-m", "add submodule"]);

    let (handle, root) = discover_repo(&outer).unwrap();
    let entries = submodules::list_submodules(&handle, &root).unwrap();
    assert_eq!(entries.len(), 1);
    let sm = &entries[0];
    assert_eq!(sm.name, "vendor/inner");
    assert!(sm.redirect_to_path.is_some());
    let target = PathBuf::from(sm.redirect_to_path.as_ref().unwrap());
    assert_eq!(target, root.join("vendor").join("inner"));

    cleanup(&outer);
    cleanup(&inner);
}
