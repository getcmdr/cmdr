//! A new backup reaches an open `Browse snapshots` pane without a refresh.
//!
//! These cells drive the real listing-open and listing-close paths over a real
//! repo on a real local drive, through the app's route and parked hub. The
//! watch's OS stream is scripted in a test binary, so a cell fires it by hand
//! (`snapshot_watch::fire`) instead of waiting on FSEvents.

// #1225: POSIX snapshot paths (`strip_prefix("/")`) and `std::path` on app paths; un-gates with app-path plan milestone 10c.
#![cfg(unix)]
#![cfg(test)]

use cmdr_fs::app_path::AppPathBuf;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cmdr_fs::entry::FileEntry;
use cmdr_fs::restic_view::VIRTUAL_ENTRY_NAME;
use cmdr_fs::volume::{DirectoryChange, UnlockOutcome, Volume};
use cmdr_listing::ListingId;
use cmdr_listing::testing::{read_to_end, request};
use cmdr_local::LocalVolume;
use cmdr_restic::test_fixtures::FixtureRepo;

use super::snapshot_watch;
use crate::file_system::volume::InMemoryVolume;
use crate::file_system::volume::manager::get_volume_manager;
use crate::listing_host::listings;
use crate::test_support::wait_until_async;

const PASSWORD: &str = "restic-live";

/// When the first backup ran, and the one a cell adds later.
const FIRST: u64 = 1_790_000_000;
const SECOND: u64 = FIRST + 3_600;

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

/// A repo with one backup of a one-file folder, on a local drive registered
/// under its own id, unlocked.
struct Repo {
    volume_id: String,
    repo: PathBuf,
    /// The backed-up file, as the view shows it under `latest`.
    latest_file: PathBuf,
    fixture: FixtureRepo,
    drive: Arc<LocalVolume>,
    _dir: tempfile::TempDir,
}

impl Repo {
    async fn build(tag: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        // The canonical spelling: restic records the path it was handed, and the
        // temp dir sits behind the `/var` → `/private/var` symlink on macOS.
        let root = std::fs::canonicalize(dir.path()).expect("canonical tempdir");
        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("source folder");
        std::fs::write(source.join("notes.txt"), b"first backup\n").expect("source file");

        let fixture = FixtureRepo::init(PASSWORD);
        fixture.backup(&source, at(FIRST));
        let drive = Arc::new(LocalVolume::local_folder("Drive", &root, crate::volume_host::host()));
        let repo = root.join("repo");
        fixture.write_into(drive.as_ref(), &repo).await;

        let volume_id = cmdr_fs::testing::unique_test_id(tag);
        get_volume_manager().register(&volume_id, Arc::clone(&drive) as Arc<dyn Volume>);
        let latest_file = repo
            .join(VIRTUAL_ENTRY_NAME)
            .join("latest")
            .join(source.strip_prefix("/").expect("an absolute source"))
            .join("notes.txt");
        let built = Self {
            volume_id,
            repo,
            latest_file,
            fixture,
            drive,
            _dir: dir,
        };
        built.unlock().await;
        built
    }

    fn view_root(&self) -> PathBuf {
        self.repo.join(VIRTUAL_ENTRY_NAME)
    }

    async fn view(&self) -> Arc<dyn Volume> {
        get_volume_manager()
            .resolve(&self.volume_id, &crate::test_support::native_app_path(self.view_root()))
            .await
            .volume
            .expect("the route serves the repo's view")
    }

    async fn unlock(&self) {
        let outcome = self
            .view()
            .await
            .as_password_locked()
            .expect("a repo's view is password-locked")
            .unlock(PASSWORD.to_string())
            .await;
        assert!(matches!(outcome, Ok(UnlockOutcome::Unlocked)), "{outcome:?}");
    }

    /// Opens `path` the way a pane does, answering the listing id to close with.
    async fn open(&self, path: &Path) -> ListingId {
        let listing_id = uuid::Uuid::new_v4().to_string();
        read_to_end(listings(), &request(&listing_id, &self.volume_id, path))
            .await
            .expect("the pane opens the listing");
        ListingId::new(listing_id)
    }

    /// A backup another app made: `restic backup` writes its files into the
    /// repo, the snapshot file last.
    async fn back_up_again(&self) {
        self.fixture.add_snapshot(at(SECOND), "laptop");
        self.fixture.write_into(self.drive.as_ref(), &self.repo).await;
    }

    async fn watch_settles_at(&self, watching: bool) {
        // Watching means its stream is armed too: the arm is detached, and a
        // scripted event before it lands would reach nothing.
        wait_until_async(Duration::from_secs(5), "the repo's snapshot watch settles", || {
            snapshot_watch::is_watching(&self.volume_id, &self.repo) == watching
                && snapshot_watch::has_stream(&self.volume_id, &self.repo) == watching
        })
        .await;
    }

    fn drop_it(self) {
        get_volume_manager().unregister(&self.volume_id);
    }
}

/// The dated snapshot folders a listing shows, `latest` left out.
fn dated_folders(listing: &ListingId) -> Vec<String> {
    listings()
        .entries(listing)
        .expect("the listing is open")
        .into_iter()
        .map(|entry| entry.name)
        .filter(|name| name != "latest")
        .collect()
}

/// When the `latest` row says its backup ran, in Unix seconds.
fn latest_backup_time(listing: &ListingId) -> Option<u64> {
    listings()
        .entries(listing)
        .expect("the listing is open")
        .into_iter()
        .find(|entry| entry.name == "latest")
        .and_then(|entry| entry.modified_at)
}

async fn shows_dated_folders(listing: &ListingId, count: usize) {
    wait_until_async(Duration::from_secs(10), "the pane re-lists the snapshots", || {
        dated_folders(listing).len() == count
    })
    .await;
}

/// The issue's case: a backup finishes while the pane sits on `Browse
/// snapshots`, and its folder appears with no refresh, `latest` moving to it.
#[tokio::test]
async fn a_new_backup_appears_in_an_open_snapshots_pane() {
    let repo = Repo::build("restic-live-appears").await;
    let pane = repo.open(&repo.view_root()).await;
    assert_eq!(dated_folders(&pane).len(), 1, "one backup so far");
    repo.watch_settles_at(true).await;

    repo.back_up_again().await;
    assert!(
        snapshot_watch::fire(&repo.volume_id, &repo.repo),
        "the pane keeps the repo's snapshots watched"
    );

    shows_dated_folders(&pane, 2).await;
    assert_eq!(
        latest_backup_time(&pane),
        Some(SECOND),
        "nothing reads under `latest`, so it moves to the new backup"
    );

    listings().end(&pane);
    repo.watch_settles_at(false).await;
    repo.drop_it();
}

/// A copy out of `latest` addresses files by path, so the same event during it
/// shows the new folder but leaves `latest` on the snapshot being read.
#[tokio::test]
async fn a_new_backup_during_a_read_under_latest_leaves_latest_alone() {
    let repo = Repo::build("restic-live-copy").await;
    let pane = repo.open(&repo.view_root()).await;
    repo.watch_settles_at(true).await;

    let view = repo.view().await;
    let mut reading = view
        .open_read_stream(&AppPathBuf::from_native_unicode(&repo.latest_file).expect("a scratch path is an app path"))
        .await
        .expect("the copy opens the file under `latest`");
    let first = reading.next_chunk().await;
    assert!(matches!(first, Some(Ok(_))), "the copy reads its first chunk");

    repo.back_up_again().await;
    assert!(snapshot_watch::fire(&repo.volume_id, &repo.repo));

    shows_dated_folders(&pane, 2).await;
    assert_eq!(
        latest_backup_time(&pane),
        Some(FIRST),
        "`latest` stays on the snapshot the copy reads"
    );

    drop(reading);
    listings().end(&pane);
    repo.watch_settles_at(false).await;
    repo.drop_it();
}

/// Two panes on one repo share one watch, which goes with the last of them; an
/// event after that reaches nothing.
#[tokio::test]
async fn the_watch_goes_when_the_last_snapshots_pane_closes() {
    let repo = Repo::build("restic-live-closes").await;
    let left = repo.open(&repo.view_root()).await;
    let right = repo.open(&repo.view_root()).await;
    repo.watch_settles_at(true).await;

    listings().end(&left);
    repo.watch_settles_at(true).await;
    assert!(
        snapshot_watch::fire(&repo.volume_id, &repo.repo),
        "the right pane still holds it"
    );

    listings().end(&right);
    repo.watch_settles_at(false).await;
    assert!(!snapshot_watch::fire(&repo.volume_id, &repo.repo), "no pane, no watch");
    repo.drop_it();
}

/// A parent whose own watcher reports changes through the host (a direct SMB
/// share's CHANGE_NOTIFY) needs no stream of ours: its report of the new
/// snapshot file re-lists the pane.
#[tokio::test]
async fn a_parents_own_change_report_relists_the_pane() {
    let repo = Repo::build("restic-live-report").await;
    let pane = repo.open(&repo.view_root()).await;
    repo.watch_settles_at(true).await;

    repo.back_up_again().await;
    crate::listing_host::directory_changed(
        &repo.volume_id,
        &repo.repo.join("snapshots"),
        DirectoryChange::FullRefresh,
    );

    shows_dated_folders(&pane, 2).await;
    listings().end(&pane);
    repo.watch_settles_at(false).await;
    repo.drop_it();
}

/// Which listings arm anything at all, asserted on the decision itself: driving
/// the open path can only prove the `true` cases. Only the view's root lists
/// snapshots, so only a pane there arms: a pane inside a snapshot, or on a real
/// folder that happens to share the row's name, keeps nothing watched.
#[tokio::test]
async fn only_a_pane_on_the_snapshots_root_arms_a_watch() {
    let repo = Repo::build("restic-live-decides").await;
    let view = repo.view().await;
    assert_eq!(
        snapshot_watch::repo_a_listing_watches(&repo.volume_id, view.as_ref(), &repo.view_root()),
        Some(repo.repo.clone()),
        "the snapshots root is the one listing a new backup changes"
    );
    assert_eq!(
        snapshot_watch::repo_a_listing_watches(&repo.volume_id, view.as_ref(), &repo.view_root().join("latest")),
        None,
        "a snapshot's own tree never changes"
    );

    let plain: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Plain"));
    assert_eq!(
        snapshot_watch::repo_a_listing_watches(&repo.volume_id, plain.as_ref(), &repo.view_root()),
        None,
        "a real folder named like the row is the parent's, not a view"
    );
    repo.drop_it();
}

/// A change reported through the composer, as a share's own watcher reports it.
fn report(volume_id: &str, parent: &Path, change: DirectoryChange) {
    crate::listing_host::directory_changed(volume_id, parent, change);
}

fn entry_in(parent: &Path, name: &str, is_dir: bool) -> FileEntry {
    FileEntry::new(
        name.to_string(),
        parent.join(name).to_string_lossy().into_owned(),
        is_dir,
        false,
    )
}

/// A direct SMB share reports every change on the share, so a busy folder
/// beside the repo, restic writing its packs and index, or a save at the share
/// root must not re-read `snapshots/`: that would be polling at network speed.
/// Only a change IN `snapshots/`, or an overflow reported on a folder above it,
/// counts.
#[tokio::test]
async fn only_a_change_in_snapshots_or_an_overflow_above_it_counts() {
    let repo = Repo::build("restic-live-unrelated").await;
    let pane = repo.open(&repo.view_root()).await;
    repo.watch_settles_at(true).await;
    let beside = repo.repo.parent().expect("the repo has a parent folder");
    let data = repo.repo.join("data");

    for i in 0..50 {
        report(
            &repo.volume_id,
            &repo.repo,
            DirectoryChange::Modified(entry_in(&repo.repo, "index", true)),
        );
        report(
            &repo.volume_id,
            &data,
            DirectoryChange::Added(entry_in(&data, "ab", true)),
        );
        let note = format!("notes-{i}.txt");
        report(
            &repo.volume_id,
            beside,
            DirectoryChange::Added(entry_in(beside, &note, false)),
        );
        report(&repo.volume_id, beside, DirectoryChange::Removed(note));
    }
    assert_eq!(
        snapshot_watch::changes_heard(&repo.volume_id, &repo.repo),
        0,
        "nothing in `snapshots/` changed"
    );

    report(&repo.volume_id, beside, DirectoryChange::FullRefresh);
    assert_eq!(
        snapshot_watch::changes_heard(&repo.volume_id, &repo.repo),
        1,
        "an overflow above the repo may hide a new snapshot"
    );
    let snapshots = repo.repo.join("snapshots");
    report(
        &repo.volume_id,
        &snapshots,
        DirectoryChange::Added(entry_in(&snapshots, "f00d", false)),
    );
    assert_eq!(snapshot_watch::changes_heard(&repo.volume_id, &repo.repo), 2);

    listings().end(&pane);
    repo.watch_settles_at(false).await;
    repo.drop_it();
}

/// A burst in `snapshots/` (restic's temp file and its rename, a `forget` of
/// many) settles into one re-list.
#[tokio::test]
async fn a_burst_in_snapshots_relists_once() {
    let repo = Repo::build("restic-live-burst").await;
    let pane = repo.open(&repo.view_root()).await;
    repo.watch_settles_at(true).await;

    repo.back_up_again().await;
    let snapshots = repo.repo.join("snapshots");
    for i in 0..20 {
        let name = format!("tmp-{i}");
        report(
            &repo.volume_id,
            &snapshots,
            DirectoryChange::Added(entry_in(&snapshots, &name, false)),
        );
    }

    shows_dated_folders(&pane, 2).await;
    wait_until_async(Duration::from_secs(5), "the re-list settles", || {
        !snapshot_watch::is_relisting(&repo.volume_id, &repo.repo)
    })
    .await;
    assert_eq!(snapshot_watch::relists(&repo.volume_id, &repo.repo), 1);

    listings().end(&pane);
    repo.watch_settles_at(false).await;
    repo.drop_it();
}
