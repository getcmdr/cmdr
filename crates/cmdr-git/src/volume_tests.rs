//! What [`GitPortalVolume`](crate::volume::GitPortalVolume) promises as a
//! `Volume`: the shared read-only conformance assertions every backend runs,
//! plus the portal-specific half of its namespace.

#![cfg(test)]

use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_fs::volume::conformance;

use crate::portal::GitPortal;
use crate::test_fixtures::{EntryKind, Fixture, cleanup, git_cli_capture, temp_dir};
use crate::volume::GitPortalVolume;
use cmdr_fs::volume::WriteMode;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{DirectoryCreation, InMemoryVolume, SpaceInfo, StreamWriteProgress, Volume, VolumeError};

/// A repo with one commit on `main` (a plain file and an executable one), a
/// second branch, and a tag, plus the portal volume serving it.
fn portal_over_a_repo(name: &str) -> (PathBuf, GitPortalVolume) {
    let dir = temp_dir("portal_volume", name);
    let mut fixture = Fixture::init(dir.clone());
    fixture.commit_files_with_modes(
        &[
            ("README.md", b"the bytes a copy would move", EntryKind::Blob),
            ("scripts/run.sh", b"#!/bin/sh\n", EntryKind::BlobExecutable),
        ],
        "initial",
        1_700_000_000,
    );
    fixture.create_branch("feature/foo");

    let parent: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Parent"));
    let portal = Arc::new(GitPortal::new(
        VolumeHost::detached(),
        crate::state_sink::no_git_state_sink(),
    ));
    let volume = GitPortalVolume::new(portal, dir.clone(), parent);
    (dir, volume)
}

/// `<repo>/.git/<rest>`, the shape every path a portal volume is asked about
/// takes (the resolve hands the input path through verbatim).
fn virtual_path(repo: &Path, rest: &str) -> PathBuf {
    repo.join(".git").join(rest)
}

/// ❗ Space at a path is the parent's answer for the filesystem holding the repo,
/// ❌ never the parent's volume-wide figure: a repo on a phone's SD card sits on
/// the card, not on the shared storage the phone's one figure describes.
#[tokio::test]
async fn space_at_a_path_is_the_parents_answer_for_the_repos_filesystem() {
    let repo = PathBuf::from("/sdcard-ext/code/app");
    let parent = InMemoryVolume::new("Parent")
        .with_space_info(1_000, 400)
        .with_space_info_under("/sdcard-ext", 500, 100);
    let portal = Arc::new(GitPortal::new(
        VolumeHost::detached(),
        crate::state_sink::no_git_state_sink(),
    ));
    let volume = GitPortalVolume::new(portal, repo.clone(), Arc::new(parent));

    let space = volume
        .get_space_info_at(&AppPath::legacy_unchecked(&virtual_path(&repo, "branches/main")))
        .await;
    assert_eq!(space.unwrap(), SpaceInfo::bounded(500, 100));
}

// ── The shared conformance assertions, from the read-only side ─────────────

/// The shared writability declaration: the portal offers no mutations, so it
/// must not claim any.
#[tokio::test]
async fn is_writable_honors_the_shared_declaration_contract() {
    let (dir, volume) = portal_over_a_repo("writability");

    conformance::assert_writability_matches_the_mutations_offered(&volume, &virtual_path(&dir, "branches/scratch"))
        .await;

    cleanup(&dir);
}

/// The shared export handshake: the portal exists to be copied OUT of, so the
/// bytes it streams and the capability it declares have to agree.
#[tokio::test]
async fn export_honors_the_shared_handshake_contract() {
    let (dir, volume) = portal_over_a_repo("export");

    conformance::assert_export_matches_the_bytes_offered(
        &volume,
        &virtual_path(&dir, "branches/main/README.md"),
        b"the bytes a copy would move",
    )
    .await;

    cleanup(&dir);
}

/// The shared dates claim: the portal claims no date beyond Modified, and no
/// row (a snapshot file, a branch, a commit) carries one, so a Created or Added
/// sort falls back to Modified here.
#[tokio::test]
async fn listing_dates_honor_the_shared_claim_contract() {
    let (dir, volume) = portal_over_a_repo("dates");

    for listed in ["branches/main", "branches", "commits"] {
        conformance::assert_listing_dates_are_claimed(&volume, &virtual_path(&dir, listed)).await;
    }
    assert_eq!(
        volume
            .supported_dates(&AppPath::legacy_unchecked(&virtual_path(&dir, "commits")))
            .await,
        cmdr_fs::volume::SupportedDates::NONE
    );

    cleanup(&dir);
}

/// The shared `NotFound` payload: a name that isn't in the snapshot reports the
/// path the caller asked for, which is what the transfer layer renders as the
/// user's own filename.
#[tokio::test]
async fn not_found_honors_the_shared_path_payload_contract() {
    let (dir, volume) = portal_over_a_repo("not_found");

    conformance::assert_not_found_carries_the_path(&volume, &virtual_path(&dir, "branches/main/no-such-file.txt"))
        .await;

    cleanup(&dir);
}

/// The shared stop assertion: a copy scan of a snapshot honors Cancel, so a
/// user who starts one over a huge branch tree can get out of it.
#[tokio::test]
async fn a_batch_scan_stops_when_it_is_told_to() {
    let (dir, volume) = portal_over_a_repo("scan_stop");

    conformance::assert_batch_scan_stops_when_told(&volume, &virtual_path(&dir, "branches/main")).await;

    cleanup(&dir);
}

/// The shared "ask inside the walk" assertion: the boundary is consulted per
/// entry, ❗ not per source path, so Cancel reaches a walk already in progress.
#[tokio::test]
async fn a_batch_scan_asks_the_boundary_inside_the_walk() {
    let (dir, volume) = portal_over_a_repo("scan_asks");

    conformance::assert_batch_scan_asks_inside_the_walk(&volume, &virtual_path(&dir, "branches/main"), 2).await;

    cleanup(&dir);
}

// ── The portal's own namespace ─────────────────────────────────────────────

/// The volume's root is `<worktree>/.git`, and listing it answers the six
/// virtual categories and NOTHING else. The real `.git/*` entries belong to the
/// parent volume, which is what keeps them writable and keeps a walker that
/// lists through this volume from meeting a name with no inode behind it.
#[tokio::test]
async fn the_root_listing_is_the_six_categories_and_no_real_entries() {
    let (dir, volume) = portal_over_a_repo("root_listing");

    assert_eq!(*volume.root().as_std_legacy(), dir.join(".git"));
    let names: Vec<String> = volume
        .list_directory(&AppPath::legacy_unchecked(&dir.join(".git")), None)
        .await
        .expect("the portal lists its own root")
        .into_iter()
        .map(|entry| entry.name)
        .collect();

    assert_eq!(
        names,
        ["branches", "tags", "commits", "stash", "worktrees", "submodules"]
    );

    cleanup(&dir);
}

/// A real `.git/*` entry is not in this volume's namespace, even though it's
/// right there on disk: the parent volume serves it, and this one says so
/// rather than inventing a row.
#[tokio::test]
async fn a_real_dot_git_entry_is_not_the_portals_to_serve() {
    let (dir, volume) = portal_over_a_repo("real_entries");

    for real in ["config", "HEAD"] {
        assert!(dir.join(".git").join(real).exists(), "{real} is on disk");
        let path = virtual_path(&dir, real);
        assert!(
            !volume.exists(&AppPath::legacy_unchecked(&path)).await,
            "{real} is not the portal's"
        );
        assert!(
            matches!(
                volume.get_metadata(&AppPath::legacy_unchecked(&path)).await,
                Err(VolumeError::NotFound(_))
            ),
            "{real} must read as not found here"
        );
    }

    cleanup(&dir);
}

/// A `.git` that isn't a repository at all answers `NotFound` instead of
/// failing to construct: routing is lexical and does no I/O, so this volume is
/// where "is there actually a repo here?" gets answered, on first use.
#[tokio::test]
async fn a_dot_git_that_is_not_a_repository_answers_not_found() {
    let dir = temp_dir("portal_volume", "not_a_repo");
    std::fs::create_dir_all(dir.join(".git")).expect("make a bare .git directory");
    std::fs::write(dir.join(".git").join("HEAD"), b"not a repo\n").expect("seed junk");

    let parent: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Parent"));
    let portal = Arc::new(GitPortal::new(
        VolumeHost::detached(),
        crate::state_sink::no_git_state_sink(),
    ));
    let volume = GitPortalVolume::new(portal, dir.clone(), parent);

    let branches = virtual_path(&dir, "branches");
    assert!(matches!(
        volume.list_directory(&AppPath::legacy_unchecked(&branches), None).await,
        Err(VolumeError::NotFound(_))
    ));
    assert!(!volume.exists(&AppPath::legacy_unchecked(&branches)).await);

    cleanup(&dir);
}

/// Browsing down a branch: the category lists both branches, and the branch
/// lists the snapshot's own tree with the executable bit intact, which is what
/// a cross-volume copy reads to set the mode on the destination.
#[tokio::test]
async fn a_branch_lists_its_snapshot_with_modes_intact() {
    let (dir, volume) = portal_over_a_repo("branch_tree");

    let branches: Vec<String> = volume
        .list_directory(&AppPath::legacy_unchecked(&virtual_path(&dir, "branches")), None)
        .await
        .expect("list branches")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert!(branches.contains(&"main".to_string()), "{branches:?}");
    assert!(branches.contains(&"feature/foo".to_string()), "{branches:?}");

    let run_sh = volume
        .get_metadata(&AppPath::legacy_unchecked(&virtual_path(
            &dir,
            "branches/main/scripts/run.sh",
        )))
        .await
        .expect("stat a blob inside the snapshot");
    assert_eq!(run_sh.permissions & 0o111, 0o111, "the executable bit survives");
    assert!(!run_sh.is_directory);
    assert!(
        volume
            .is_directory(&AppPath::legacy_unchecked(&virtual_path(&dir, "branches/main/scripts")))
            .await
            .expect("stat the tree"),
    );

    cleanup(&dir);
}

/// A whole branch tree can be scanned for a copy, so pasting one into another
/// volume has real totals to plan with rather than a refusal.
#[tokio::test]
async fn a_snapshot_subtree_scans_for_a_copy() {
    let (dir, volume) = portal_over_a_repo("copy_scan");

    let scanned = volume
        .scan_for_copy(&AppPath::legacy_unchecked(&virtual_path(&dir, "branches/main")))
        .await
        .expect("scan a branch tree");

    // `README.md` and `scripts/run.sh`, under the snapshot root and `scripts/`
    // (the shared walk counts the source directory itself).
    assert_eq!(scanned.file_count, 2, "{scanned:?}");
    assert_eq!(scanned.dir_count, 2, "{scanned:?}");
    assert!(scanned.total_bytes > 0, "{scanned:?}");

    cleanup(&dir);
}

/// Every mutation refuses, including the recursive create whose trait default
/// would answer `Ok` for a directory that's already there.
#[tokio::test]
async fn every_mutation_is_unsupported() {
    let (dir, volume) = portal_over_a_repo("mutations");
    let existing = virtual_path(&dir, "branches");
    let blob = virtual_path(&dir, "branches/main/README.md");

    assert!(matches!(
        volume.create_file(&AppPath::legacy_unchecked(&blob), b"x").await,
        Err(VolumeError::NotSupported)
    ));
    assert!(matches!(
        volume.create_directory(&AppPath::legacy_unchecked(&existing)).await,
        Err(VolumeError::NotSupported)
    ));
    assert!(
        matches!(
            volume.create_directory_all(&AppPath::legacy_unchecked(&existing)).await,
            Err(VolumeError::NotSupported)
        ),
        "the trait default would answer {:?} for a directory that exists",
        DirectoryCreation::AlreadyExisted
    );
    assert!(matches!(
        volume.delete(&AppPath::legacy_unchecked(&blob)).await,
        Err(VolumeError::NotSupported)
    ));
    assert!(matches!(
        volume
            .rename(
                &AppPath::legacy_unchecked(&blob),
                &AppPath::legacy_unchecked(&virtual_path(&dir, "branches/main/other.md")),
                false
            )
            .await,
        Err(VolumeError::NotSupported)
    ));

    cleanup(&dir);
}

/// The punchline the whole routing design exists for: a copy OUT of a virtual
/// `.git/branches/main/…` path into another volume. The bytes must match
/// `git show` exactly, and the entry must carry the executable bit the copy
/// engine sets on the destination.
#[tokio::test]
async fn a_copy_out_of_a_snapshot_carries_the_bytes_and_the_executable_bit() {
    use std::ops::ControlFlow;
    use std::sync::atomic::{AtomicU64, Ordering};

    let (dir, src) = portal_over_a_repo("copy_out");
    // The destination is any other volume, which is the whole point: the copy
    // engine moves bytes between two `Volume`s and neither end is special.
    let dst = InMemoryVolume::new("dst");

    let src_path = virtual_path(&dir, "branches/main/scripts/run.sh");
    let stream = src
        .open_read_stream(&AppPath::legacy_unchecked(&src_path))
        .await
        .expect("open virtual blob");
    let total = stream.total_size();

    let dest_rel = Path::new("run.sh");
    let counter = AtomicU64::new(0);
    let on_progress = |progress: StreamWriteProgress| -> ControlFlow<()> {
        counter.store(progress.bytes_written, Ordering::SeqCst);
        ControlFlow::Continue(())
    };
    let written = dst
        .write_from_stream(
            &AppPath::legacy_unchecked(dest_rel),
            WriteMode::CreateOrReplace,
            total,
            stream,
            &on_progress,
        )
        .await
        .expect("write_from_stream");
    assert_eq!(written.bytes, total.known().expect("git blobs have a known length"));
    assert_eq!(
        counter.load(Ordering::SeqCst),
        total.known().unwrap(),
        "progress reports every byte"
    );

    // What landed has to be what `git show main:scripts/run.sh` prints.
    let mut landed = dst
        .open_read_stream(&AppPath::legacy_unchecked(dest_rel))
        .await
        .expect("read the copy back");
    let mut actual = Vec::new();
    while let Some(chunk) = landed.next_chunk().await {
        actual.extend_from_slice(&chunk.expect("chunk"));
    }
    let expected = git_cli_capture(&dir, &["show", "main:scripts/run.sh"]);
    assert_eq!(actual, expected, "bytes must match git show");

    // `write_from_stream` doesn't carry the mode itself — that's the copy
    // engine's job, layered on the `FileEntry.permissions` this volume answers
    // with. So what this owes is the DATA that engine needs.
    let entry = src
        .get_metadata(&AppPath::legacy_unchecked(&src_path))
        .await
        .expect("get_metadata virtual");
    assert_eq!(
        entry.permissions & 0o111,
        0o111,
        "virtual entry must carry executable bit"
    );

    cleanup(&dir);
}

/// A path that simply isn't in the snapshot reads as "not there", ❌ never as
/// "this repo is damaged". `gix`'s tree walk answers `Ok(None)` for a name it
/// couldn't find, which is an ordinary miss (a typo in the path bar, a file
/// that only exists on another branch) and has to stay distinct from the `Err`
/// that means the object database couldn't answer at all. Every entry point
/// gives the same answer.
#[tokio::test]
async fn a_path_missing_from_a_snapshot_reads_as_not_found() {
    let (dir, volume) = portal_over_a_repo("missing_paths");

    for missing in [
        "branches/main/no-such-file.txt",
        "branches/main/no-such-dir/inner.txt",
        "branches/no-such-branch",
        "tags/no-such-tag/README.md",
    ] {
        let path = virtual_path(&dir, missing);
        let answered = volume.get_metadata(&AppPath::legacy_unchecked(&path)).await;
        assert!(
            matches!(answered, Err(VolumeError::NotFound(ref carried)) if carried.contains("no-such")),
            "{missing}: {answered:?}",
        );
    }

    // Listing one is the same answer.
    let listed = volume
        .list_directory(
            &AppPath::legacy_unchecked(&virtual_path(&dir, "branches/main/no-such-dir")),
            None,
        )
        .await;
    assert!(matches!(listed, Err(VolumeError::NotFound(_))), "{listed:?}");

    // And so is opening one for read.
    let opened = volume
        .open_read_stream(&AppPath::legacy_unchecked(&virtual_path(
            &dir,
            "branches/main/no-such-file.txt",
        )))
        .await;
    assert!(
        matches!(opened, Err(VolumeError::NotFound(_))),
        "opening a missing blob"
    );

    cleanup(&dir);
}

/// The capability answers a routed read-only volume owes, in one place: no
/// watch (nothing on disk to arm one on), no local-FS access (a blob lives in a
/// pack file), no space poll, and the parent's lane so portal reads can't run
/// beside other work on the same disk.
#[tokio::test]
async fn the_capability_answers_match_a_routed_read_only_volume() {
    let (dir, volume) = portal_over_a_repo("capabilities");
    let parent = InMemoryVolume::new("Parent");

    assert!(!volume.can_watch_listings());
    assert_eq!(
        volume.listing_watch_coverage(&AppPath::legacy_unchecked(&virtual_path(&dir, "branches"))),
        cmdr_fs::volume::WatchCoverage::None
    );
    assert!(!volume.supports_local_fs_access());
    assert_eq!(volume.local_path(), None);
    assert_eq!(volume.space_poll_interval(), None);
    assert_eq!(volume.lane_key(), parent.lane_key());
    assert!(volume.capabilities().can_export);
    assert!(!volume.capabilities().backend_can_write);
    // Reports the mode and keeps nothing. The compress planner trusts a listed
    // mode only from a backend that reports one, and the app's loss-note matrix
    // runs this shape as `MODE_REPORTER`.
    assert_eq!(volume.metadata_caps(), cmdr_fs::metadata::MetadataCaps::MODE_REPORTER);

    cleanup(&dir);
}

// ── The commit log: progress and counts filled in later ───────────────────

/// A portal over a fresh `commits`-commit repo, and the portal itself so a cell
/// can ask it for rows directly.
fn portal_over_history(name: &str, commits: usize) -> (PathBuf, Arc<GitPortal>, GitPortalVolume) {
    let (dir, _fixture) = crate::test_fixtures::build_simple_repo(name, commits);
    let portal = Arc::new(GitPortal::new(
        VolumeHost::detached(),
        crate::state_sink::no_git_state_sink(),
    ));
    let parent: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Parent"));
    let volume = GitPortalVolume::new(Arc::clone(&portal), dir.clone(), parent);
    (dir, portal, volume)
}

#[tokio::test]
async fn the_commit_listing_reports_every_commit_as_progress() {
    let (dir, _portal, volume) = portal_over_history("commit_progress", 4);
    let seen = std::sync::Mutex::new(Vec::new());
    let on_progress = |p: cmdr_fs::volume::ListingProgress| seen.lock().unwrap().push(p.dirs);

    let entries = volume
        .list_directory(
            &AppPath::legacy_unchecked(&virtual_path(&dir, "commits")),
            Some(&on_progress),
        )
        .await
        .unwrap();

    assert_eq!(entries.len(), 4);
    assert_eq!(
        seen.lock().unwrap().last(),
        Some(&4),
        "the last tick is the exact count"
    );
    cleanup(&dir);
}

#[tokio::test]
async fn the_portal_fills_in_counts_for_commit_rows_only() {
    let (dir, portal, volume) = portal_over_history("commit_fill", 2);
    let commits = volume
        .list_directory(&AppPath::legacy_unchecked(&virtual_path(&dir, "commits")), None)
        .await
        .unwrap();
    let mut paths: Vec<PathBuf> = commits.iter().map(|e| PathBuf::from(&e.path)).collect();
    paths.push(virtual_path(&dir, "branches/main"));
    paths.push(dir.join("README.md"));

    let filled = portal.commit_rows_with_counts(&paths);

    assert_eq!(filled.len(), 2, "the branch row and the real file are left out");
    for (row, listed) in filled.iter().zip(&commits) {
        assert_eq!(row.path, listed.path);
        assert!(
            matches!(row.git_meta, Some(cmdr_fs::git_meta::GitEntryMeta::Count { .. })),
            "{} came back without its count",
            row.name
        );
    }
    cleanup(&dir);
}
