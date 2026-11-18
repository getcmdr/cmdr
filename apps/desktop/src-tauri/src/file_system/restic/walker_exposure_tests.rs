//! What the `Browse snapshots` row exposes to the walkers that are NOT a pane:
//! the delete walkers, the copy scan, the fresh-listing oracle, the drive
//! index, and on-demand folder sizes.
//!
//! The row is a name with no inode behind it, and the view under it holds
//! every snapshot's logical files: 100 snapshots of a 500 GB Mac are about
//! 50 TB. These cells pin that NO walker but a pane meets it. The row reaches a
//! pane through `crates/cmdr-listing/src/host/overlays.rs`, and every other
//! reader lists through `Volume`, which doesn't hold it.

#![cfg(unix)] // POSIX-only cells: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan, task 35 and #1225).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::file_system::volume::{InMemoryVolume, StreamLength, Volume, WriteMode};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::restic_view::VIRTUAL_ENTRY_NAME;
use cmdr_listing::testing::TestListing;
use cmdr_local::LocalVolume;
use cmdr_restic::test_fixtures::FixtureRepo;

/// A real (empty) restic repo at `<tempdir>/repo`, on a local volume rooted at
/// the tempdir.
async fn local_repo() -> (tempfile::TempDir, PathBuf, Arc<dyn Volume>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let volume: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Test",
        dir.path(),
        crate::volume_host::host(),
    ));
    let repo = dir.path().join("repo");
    FixtureRepo::init("pw").write_into(volume.as_ref(), &repo).await;
    (dir, repo, volume)
}

fn names(entries: &[FileEntry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

/// The volume's own answer for the repo folder is what's on disk, nothing more.
#[tokio::test]
async fn a_volume_listing_of_the_repo_carries_no_row() {
    let (_dir, repo, volume) = local_repo().await;

    let entries = volume.list_directory(&repo, None).await.expect("lists the repo");
    assert!(names(&entries).contains(&"config"), "{:?}", names(&entries));
    assert!(!names(&entries).contains(&VIRTUAL_ENTRY_NAME), "{:?}", names(&entries));
}

/// A PANE's listing of the same folder: the real entries plus the one row.
#[tokio::test]
async fn a_decorated_listing_of_the_repo_shows_the_row() {
    let (_dir, repo, volume) = local_repo().await;

    let mut entries = volume.list_directory(&repo, None).await.expect("lists the repo");
    let added = cmdr_listing::testing::decorate(
        crate::listing_host::listings(),
        "walker-decorated",
        &volume,
        &repo,
        &mut entries,
    )
    .await;
    assert_eq!(added, 1);
    assert!(names(&entries).contains(&VIRTUAL_ENTRY_NAME), "{:?}", names(&entries));
    assert!(names(&entries).contains(&"config"), "real entries stay");
}

/// The volume-aware delete walker lists through `Volume`, so it never meets
/// the row, and the whole repo comes off.
#[tokio::test]
async fn the_delete_walker_never_meets_the_row_and_deletes_the_repo() {
    let (_dir, repo, volume) = local_repo().await;

    let mut stack = vec![repo.clone()];
    let mut directories = Vec::new();
    while let Some(next) = stack.pop() {
        for child in volume.list_directory(&next, None).await.expect("listing a real dir") {
            assert_ne!(child.name, VIRTUAL_ENTRY_NAME, "the walker met the row");
            let path = next.join(&child.name);
            if child.is_directory {
                stack.push(path);
            } else {
                volume.delete(&path).await.expect("a real repo file deletes");
            }
        }
        directories.push(next);
    }
    for directory in directories.into_iter().rev() {
        volume.delete(&directory).await.expect("a real repo folder deletes");
    }
    assert!(!repo.exists(), "the repo is gone");
}

/// The repo's real files are ordinary files: readable, copyable, writable,
/// renamable, deletable. Nothing in the local backend knows the word restic.
#[tokio::test]
async fn real_repo_files_stay_fully_mutable() {
    let (_dir, repo, volume) = local_repo().await;
    let config = repo.join("config");

    let source = volume.open_read_stream(&config).await.expect("config streams");
    let size = std::fs::metadata(&config).expect("config is on disk").len();
    volume
        .write_from_stream(
            &repo.join("config.bak"),
            WriteMode::CreateOrReplace,
            StreamLength::Known(size),
            source,
            &|_| std::ops::ControlFlow::Continue(()),
        )
        .await
        .expect("writing beside it lands");
    volume
        .rename(&repo.join("config.bak"), &repo.join("config.old"), false)
        .await
        .expect("renaming in the repo");
    volume
        .delete(&repo.join("config.old"))
        .await
        .expect("deleting in the repo");
    volume.delete(&config).await.expect("deleting the config itself");
    assert!(!config.exists());
}

/// The copy scan of the repo folder counts what's on disk: no row, no view.
#[tokio::test]
async fn the_copy_scan_counts_only_what_is_on_disk() {
    let (_dir, repo, volume) = local_repo().await;

    let scanned = volume
        .scan_for_copy_batch(std::slice::from_ref(&repo))
        .await
        .expect("scanning the repo succeeds");

    let (mut dirs, mut files) = (0usize, 0usize);
    for entry in walkdir::WalkDir::new(&repo).min_depth(1) {
        let entry = entry.expect("walks");
        if entry.file_type().is_dir() {
            dirs += 1;
        } else if entry.file_type().is_file() {
            files += 1;
        }
    }
    assert_eq!(scanned.aggregate.file_count as usize, files);
    assert_eq!(scanned.aggregate.dir_count as usize, dirs);
}

/// A pane's decorated listing of the repo never stands in for a walker's read.
#[tokio::test]
async fn the_listing_oracle_declines_a_decorated_repo_listing() {
    let (_dir, repo, volume) = local_repo().await;
    let mut cached = volume.list_directory(&repo, None).await.expect("lists the repo");
    let added = cmdr_listing::testing::decorate(
        crate::listing_host::listings(),
        crate::file_system::volume::DEFAULT_VOLUME_ID,
        &volume,
        &repo,
        &mut cached,
    )
    .await;
    assert_eq!(added, 1);

    let _guard = TestListing::new()
        .volume(crate::file_system::volume::DEFAULT_VOLUME_ID)
        .path(repo.clone())
        .entries(cached)
        .overlay_rows(added)
        .insert(crate::listing_host::listings(), "restic_oracle_decorated");

    assert!(
        crate::listing_host::listings()
            .authoritative_listing(crate::file_system::volume::DEFAULT_VOLUME_ID, &repo)
            .is_none()
    );
}

/// A cached listing UNDER the row that the sync resolve couldn't route (the
/// hub hasn't confirmed the repo) is never vouched for by the parent's watch:
/// it's a view listing, and the parent's coverage says nothing about it.
#[tokio::test]
async fn the_listing_oracle_declines_an_unrouted_path_under_the_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let inside = dir.path().join("repo").join(VIRTUAL_ENTRY_NAME).join("latest");
    let volume_id = "walker-oracle-unrouted";
    // A parent whose watch covers every writer: the coverage that WOULD vouch
    // for a real folder's cached listing.
    let _volume = crate::file_system::volume::manager::test_support::TestVolumeRegistration::install(
        volume_id,
        Arc::new(cmdr_listing::testing::WatchCoverageVolume::new(
            "Disk",
            crate::file_system::volume::WatchCoverage::EveryWriter,
        )),
    );
    let _guard = TestListing::new()
        .volume(volume_id)
        .path(inside.clone())
        .entries(vec![FileEntry::new(
            "Users".to_string(),
            inside.join("Users").to_string_lossy().into_owned(),
            true,
            false,
        )])
        .insert(crate::listing_host::listings(), "restic_oracle_unrouted");

    assert!(
        crate::listing_host::listings()
            .authoritative_listing(volume_id, &inside)
            .is_none(),
        "an unconfirmed view path is never answered from the cache"
    );
}

/// The drive index's walk of a share holding a repo records the repo's real
/// files and never a path through the view.
#[tokio::test]
async fn the_drive_index_walk_never_records_the_view() {
    use cmdr_index::store::{IndexStore, ROOT_ID};
    use cmdr_index::testing::scan::{IndexWriter, ScanPacer, ScanProgress, scan_volume_via_trait};

    let share = InMemoryVolume::new("Share");
    FixtureRepo::init("pw")
        .write_into(&share, Path::new("/share/repo"))
        .await;
    let share: Arc<dyn Volume> = Arc::new(share);
    // A pane listed the repo first, so the hub knows it's one.
    super::wiring::repos().overlay_row("walker-index", Path::new("/share/repo"));

    let db_dir = tempfile::tempdir().expect("db dir");
    let db_path = db_dir.path().join("index.db");
    let _store = IndexStore::open(&db_path).expect("open store");
    let writer = IndexWriter::spawn(&db_path, cmdr_index::NoopEventSink::shared()).expect("writer");
    scan_volume_via_trait(
        share,
        PathBuf::from("/share"),
        writer.clone(),
        Arc::new(ScanProgress::new()),
        tokio_util::sync::CancellationToken::new(),
        ScanPacer::unpaced(),
    )
    .await
    .expect("the scan completes");
    writer.flush().await.expect("flush");
    writer.shutdown();

    let store = IndexStore::open(&db_path).expect("reopen");
    let repo = store
        .list_children(ROOT_ID)
        .expect("root")
        .into_iter()
        .find(|entry| entry.name == "repo")
        .expect("the repo folder is indexed");
    let in_repo: Vec<String> = store
        .list_children(repo.id)
        .expect("repo children")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert!(in_repo.iter().any(|name| name == "config"), "{in_repo:?}");
    assert!(!in_repo.iter().any(|name| name == VIRTUAL_ENTRY_NAME), "{in_repo:?}");
}

/// On-demand folder sizes (⌥⇧⏎) plan the pane's folder rows, and the row is one
/// of them; it isn't a folder on the repo's volume, so the plan leaves it out
/// rather than walking 50 TB or reporting it unreadable. Space ON the row is a
/// person asking, and stays theirs.
#[tokio::test]
async fn folder_size_counting_skips_the_row_unless_asked_for_it() {
    use crate::listing_index_sizes::count::plan;

    let (_dir, repo, volume) = local_repo().await;
    let mut entries = volume.list_directory(&repo, None).await.expect("lists the repo");
    let added = cmdr_listing::testing::decorate(
        crate::listing_host::listings(),
        "walker-count",
        &volume,
        &repo,
        &mut entries,
    )
    .await;
    assert_eq!(added, 1);
    let row = repo.join(VIRTUAL_ENTRY_NAME).to_string_lossy().into_owned();

    let listing = TestListing::new()
        .volume("walker-count")
        .path(repo.clone())
        .entries(entries)
        .overlay_rows(added)
        .insert(crate::listing_host::listings(), "restic_count_plan");

    let planned: Vec<String> = plan(listing.id(), true, None, |_| false)
        .expect("cached")
        .folders
        .into_iter()
        .map(|folder| folder.path)
        .collect();
    assert!(planned.iter().any(|path| path.ends_with("/snapshots")), "{planned:?}");
    assert!(!planned.contains(&row), "{planned:?}");

    let asked = plan(listing.id(), true, Some(std::slice::from_ref(&row)), |_| false).expect("cached");
    assert_eq!(asked.folders.len(), 1, "Space on the row counts it");
}
