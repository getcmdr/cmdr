//! What [`ResticVolume`] promises as a `Volume`: the shared read-only
//! conformance assertions every backend runs, plus the snapshot view's own
//! namespace, reads, and `latest` pin. Every repo here is a real one, built by
//! rustic and read back through a remote-style parent (no local file access).

use super::ResticVolume;
use crate::hub::{ResticRepos, Tuning};
use crate::path::VIRTUAL_ENTRY_NAME;
use crate::test_fixtures::{self, At, FixtureRepo, app, path_in, std_path_of};
use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{BackendKind, InMemoryVolume, LockedContainer, Volume, VolumeError, conformance};
use std::fs::File;
#[cfg(unix)] // The executable bit only POSIX stores.
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

const PASSWORD: &str = "correct horse";
const ROOT: &str = "/Volumes/testshare/backups/test-restic";
const VOLUME_ID: &str = "smb-testshare";
/// 2001-09-09: old enough that a stream reporting "now" can't pass for it.
const OLD_DATE: u64 = 1_000_000_000;
const LONG_NAME_LEN: usize = 255;

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn root() -> AppPathBuf {
    app(ROOT)
}

fn view() -> AppPathBuf {
    path_in(&root(), VIRTUAL_ENTRY_NAME)
}

/// A folder to back up: a plain file, a bigger one spanning several chunks, an
/// executable, an empty folder, a symlink, and the odd names a Mac really has.
fn source_folder() -> TestDir {
    let source = TestDir::new("restic_volume_source");
    std::fs::create_dir_all(source.join("nested")).expect("source dir");
    std::fs::create_dir_all(source.join("empty")).expect("empty dir");
    std::fs::write(source.join("a.txt"), b"hello").expect("source file");
    std::fs::write(source.join("nested/b.bin"), big_bytes()).expect("source file");
    std::fs::write(source.join("run.sh"), b"#!/bin/sh\n").expect("script");
    #[cfg(unix)]
    std::fs::set_permissions(source.join("run.sh"), std::fs::Permissions::from_mode(0o755)).expect("chmod");
    std::fs::write(source.join("Cafe\u{301}.txt"), b"nfd").expect("NFD name");
    std::fs::write(source.join("with  two spaces.txt"), b"spaces").expect("spaced name");
    std::fs::write(source.join("x".repeat(LONG_NAME_LEN)), b"long").expect("long name");
    // Windows creates a link only with Developer Mode or elevation; without either
    // the backup has no link, and the link cells skip themselves.
    if cmdr_fs::testing::can_create_symlinks() {
        cmdr_fs::testing::symlink("a.txt", source.join("link")).expect("symlink");
    }
    for file in ["a.txt", "nested/b.bin", "run.sh"] {
        File::options()
            .write(true)
            .open(source.join(file))
            .and_then(|f| f.set_modified(at(OLD_DATE)))
            .expect("date the source file");
    }
    source
}

fn big_bytes() -> Vec<u8> {
    (0..300_000u32).map(|i| (i % 251) as u8).collect()
}

/// A repo on a remote-style parent, its hub, and the view over it, unlocked.
struct Browse {
    source: TestDir,
    fixture: FixtureRepo,
    parent: Arc<InMemoryVolume>,
    hub: Arc<ResticRepos>,
    volume: ResticVolume,
}

impl Browse {
    async fn new() -> Self {
        Self::with_tuning(Tuning::default()).await
    }

    async fn with_tuning(tuning: Tuning) -> Self {
        let source = source_folder();
        let fixture = FixtureRepo::init(PASSWORD);
        fixture.backup(&source, at(1_791_000_000));
        let parent = Arc::new(InMemoryVolume::new("testshare"));
        fixture.write_into_at(parent.as_ref(), &root()).await;
        let hub = Arc::new(ResticRepos::with_tuning(VolumeHost::detached(), tuning));
        let volume = view_over(&hub, &parent);
        assert!(
            test_fixtures::unlock(&volume, PASSWORD).await,
            "the right password unlocks"
        );
        Self {
            source,
            fixture,
            parent,
            hub,
            volume,
        }
    }

    /// Backs the source up again (after a test changed it) and puts the new
    /// repo files on the parent.
    async fn backup_again(&self, time: u64) {
        self.fixture.backup(&self.source, at(time));
        self.fixture.write_into_at(self.parent.as_ref(), &root()).await;
    }

    /// Where the backed-up folder sits under `snapshot` (a snapshot of a Mac
    /// starts at `/`, so the source's own absolute path is inside it).
    async fn source_in(&self, snapshot: &str) -> AppPathBuf {
        let mut dir = path_in(&view(), snapshot);
        loop {
            let entries = self.volume.list_directory(&dir, None).await.expect("lists");
            if entries.iter().any(|e| e.name == "a.txt") {
                return dir;
            }
            let [only] = entries.as_slice() else {
                panic!("{dir:?} should hold one folder on the way down: {entries:?}");
            };
            dir = path_in(&dir, &only.name);
        }
    }
}

fn view_over(hub: &Arc<ResticRepos>, parent: &Arc<InMemoryVolume>) -> ResticVolume {
    let parent: Arc<dyn Volume> = Arc::clone(parent) as Arc<dyn Volume>;
    ResticVolume::new_at(Arc::clone(hub), VOLUME_ID, parent, root())
}

fn named<'e>(entries: &'e [FileEntry], name: &str) -> &'e FileEntry {
    entries
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("{name:?} is listed: {entries:?}"))
}

async fn read_all(volume: &ResticVolume, path: &AppPath) -> Vec<u8> {
    let mut stream = volume.open_read_stream(path).await.expect("opens");
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        out.extend_from_slice(&chunk.expect("a chunk"));
    }
    out
}

// ── Unlocking ──────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn without_a_password_the_view_asks_for_one() {
    let fixture = FixtureRepo::init(PASSWORD);
    fixture.add_snapshot(at(1_791_000_000), "test-host");
    let parent = Arc::new(InMemoryVolume::new("testshare"));
    fixture.write_into_at(parent.as_ref(), &root()).await;
    let hub = Arc::new(ResticRepos::new(VolumeHost::detached()));
    let volume = view_over(&hub, &parent);

    let listing = volume.list_directory(&view(), None).await;
    assert!(
        matches!(
            listing,
            Err(VolumeError::NeedsPassword {
                container: LockedContainer::ResticRepo,
                wrong_attempt: false,
            })
        ),
        "got {listing:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_password_unlocks_nothing_and_keeps_nothing() {
    let fixture = FixtureRepo::init(PASSWORD);
    fixture.add_snapshot(at(1_791_000_000), "test-host");
    let parent = Arc::new(InMemoryVolume::new("testshare"));
    fixture.write_into_at(parent.as_ref(), &root()).await;
    let hub = Arc::new(ResticRepos::new(VolumeHost::detached()));
    let volume = view_over(&hub, &parent);

    assert!(!test_fixtures::unlock(&volume, "incorrect horse").await);
    let listing = volume.list_directory(&view(), None).await;
    assert!(
        matches!(
            listing,
            Err(VolumeError::NeedsPassword {
                container: LockedContainer::ResticRepo,
                wrong_attempt: false,
            })
        ),
        "a refused password isn't kept: got {listing:?}"
    );
    assert!(test_fixtures::unlock(&volume, PASSWORD).await);
    assert_eq!(volume.list_directory(&view(), None).await.expect("lists").len(), 2);
}

// ── The snapshot list ──────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn the_view_lists_one_folder_per_snapshot_plus_latest() {
    let browse = Browse::new().await;
    std::fs::write(browse.source.join("a.txt"), b"second").expect("change the source");
    browse.backup_again(1_792_000_000).await;

    let entries = browse.volume.list_directory(&view(), None).await.expect("lists");

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names.len(), 3, "two snapshots and latest: {names:?}");
    assert_eq!(
        names[2], "latest",
        "sorted by name, and every dated name starts with a digit"
    );
    assert!(entries.iter().all(|e| e.is_directory));
    assert_eq!(entries[0].modified_at, Some(1_791_000_000));
    assert_eq!(entries[1].modified_at, Some(1_792_000_000));
    assert_eq!(named(&entries, "latest").modified_at, Some(1_792_000_000));
    // A backup's summary carries the bytes it processed: the row's size.
    assert!(
        entries[0].size.is_some_and(|size| size > 300_000),
        "got {:?}",
        entries[0].size
    );
    // A folder's Size cell reads `recursive_size`, so the row carries both.
    assert_eq!(entries[0].recursive_size, entries[0].size);
    for entry in &entries {
        assert_eq!(entry.path, view().at(&entry.name).as_str());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_without_a_summary_lists_without_a_size() {
    let fixture = FixtureRepo::init(PASSWORD);
    fixture.add_snapshot(at(1_791_000_000), "test-host");
    let parent = Arc::new(InMemoryVolume::new("testshare"));
    fixture.write_into_at(parent.as_ref(), &root()).await;
    let hub = Arc::new(ResticRepos::new(VolumeHost::detached()));
    let volume = view_over(&hub, &parent);
    assert!(test_fixtures::unlock(&volume, PASSWORD).await);

    let entries = volume.list_directory(&view(), None).await.expect("lists");
    assert!(entries.iter().all(|e| e.size.is_none()), "{entries:?}");
}

/// The host, tags, and paths a snapshot records ride on its row as facts, on
/// the dated folder and on `latest` alike, from a listing and a stat both.
#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_row_carries_its_host_tags_and_paths() {
    let fixture = FixtureRepo::init(PASSWORD);
    fixture.add_tagged_snapshot(at(1_791_000_000), "test-host", &["daily"], &["/Users/test_user_3"]);
    fixture.add_tagged_snapshot(
        at(1_792_000_000),
        "testnas",
        &["weekly", "nas"],
        &["/share/photos", "/share/docs"],
    );
    let parent = Arc::new(InMemoryVolume::new("testshare"));
    fixture.write_into_at(parent.as_ref(), &root()).await;
    let hub = Arc::new(ResticRepos::new(VolumeHost::detached()));
    let volume = view_over(&hub, &parent);
    assert!(test_fixtures::unlock(&volume, PASSWORD).await);

    let entries = volume.list_directory(&view(), None).await.expect("lists");

    let older = entries[0]
        .restic_meta
        .as_ref()
        .expect("a dated row carries its snapshot's facts");
    assert_eq!(older.hostname, "test-host");
    assert_eq!(older.tags, vec!["daily"]);
    assert_eq!(older.paths, vec!["/Users/test_user_3"]);
    let newer = entries[1].restic_meta.as_ref().expect("every dated row does");
    assert_eq!(newer.hostname, "testnas");
    assert_eq!(newer.tags, vec!["nas", "weekly"], "sorted");
    assert_eq!(newer.paths, vec!["/share/docs", "/share/photos"], "sorted");
    assert_eq!(
        named(&entries, "latest").restic_meta.as_ref(),
        Some(newer),
        "latest states the snapshot it shows"
    );

    let stat = volume.get_metadata(&view().at(&entries[0].name)).await.expect("stats");
    assert_eq!(
        stat.restic_meta.as_ref(),
        Some(older),
        "a stat says what the listing says"
    );
    let root = volume.get_metadata(&view()).await.expect("stats the view");
    assert_eq!(root.restic_meta, None, "the view itself is no snapshot");
}

/// ❗ The time in a folder name is local time, so a saved path made in another
/// time zone (or across DST) carries a different prefix. It still finds its
/// snapshot, by the id.
#[tokio::test(flavor = "multi_thread")]
async fn a_stale_time_prefix_still_finds_its_snapshot() {
    let browse = Browse::new().await;
    let entries = browse.volume.list_directory(&view(), None).await.expect("lists");
    let dated = &entries[0].name;
    let suffix = &dated[dated.rfind('(').expect("an id suffix")..];

    let stale = view().at(format!("1999-01-01 00.00.00 {suffix}"));
    let listed = browse.volume.list_directory(&stale, None).await.expect("lists by id");
    let current = browse
        .volume
        .list_directory(&view().at(dated), None)
        .await
        .expect("lists");
    assert_eq!(
        listed.iter().map(|e| &e.name).collect::<Vec<_>>(),
        current.iter().map(|e| &e.name).collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_name_that_names_no_snapshot_is_not_found() {
    let browse = Browse::new().await;
    for name in ["nope", "2026-10-12 21.00.03 (ffffffff)"] {
        let result = browse.volume.list_directory(&view().at(name), None).await;
        assert!(
            matches!(result, Err(VolumeError::NotFound(_))),
            "{name}: got {result:?}"
        );
    }
}

// ── Trees ──────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_tree_lists_files_folders_links_and_odd_names() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;

    let entries = browse.volume.list_directory(&dir, None).await.expect("lists");

    assert_eq!(named(&entries, "a.txt").size, Some(5));
    assert_eq!(named(&entries, "a.txt").modified_at, Some(OLD_DATE));
    assert!(named(&entries, "nested").is_directory);
    assert!(named(&entries, "empty").is_directory);
    if cmdr_fs::testing::can_create_symlinks() {
        let link = named(&entries, "link");
        assert!(link.is_symlink && !link.is_directory, "a link is a row, never followed");
    }
    named(&entries, "Cafe\u{301}.txt");
    named(&entries, "with  two spaces.txt");
    named(&entries, &"x".repeat(LONG_NAME_LEN));
    #[cfg(unix)] // The fixture set the executable bit only where POSIX stores it.
    assert_eq!(named(&entries, "run.sh").permissions, 0o755);
    assert_eq!(
        named(&entries, "a.txt").permissions & 0o777,
        named(&entries, "a.txt").permissions
    );
    for entry in &entries {
        assert_eq!(entry.path, dir.at(&entry.name).as_str());
    }

    assert!(
        browse
            .volume
            .list_directory(&dir.at("empty"), None)
            .await
            .expect("lists")
            .is_empty()
    );
    let nested = browse
        .volume
        .list_directory(&dir.at("nested"), None)
        .await
        .expect("lists");
    assert_eq!(named(&nested, "b.bin").size, Some(300_000));
}

#[tokio::test(flavor = "multi_thread")]
async fn metadata_answers_for_the_view_a_snapshot_and_a_file() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;

    let top = browse.volume.get_metadata(&view()).await.expect("the view");
    assert!(top.is_directory);
    assert_eq!(top.name, VIRTUAL_ENTRY_NAME);
    let latest = browse.volume.get_metadata(&view().at("latest")).await.expect("latest");
    assert!(latest.is_directory);
    assert_eq!(latest.modified_at, Some(1_791_000_000));
    let file = browse.volume.get_metadata(&dir.at("a.txt")).await.expect("a file");
    assert_eq!(file.size, Some(5));
    assert!(!file.is_directory);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_listing_answers_cancelled() {
    let browse = Browse::new().await;
    let token = CancellationToken::new();
    token.cancel();
    let result = browse
        .volume
        .list_directory_with_cancel(&view().at("latest"), None, Some(&token))
        .await;
    assert!(matches!(result, Err(VolumeError::Cancelled(_))), "got {result:?}");
}

// ── Reads ──────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn a_file_streams_its_bytes_date_and_mode() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;

    conformance::assert_export_matches_the_bytes_offered(&browse.volume, &std_path_of(&dir.at("a.txt")), b"hello")
        .await;
    conformance::assert_read_stream_reports_the_listed_date(&browse.volume, &std_path_of(&dir.at("a.txt"))).await;
    conformance::assert_read_stream_reports_the_listed_metadata(&browse.volume, &std_path_of(&dir.at("run.sh"))).await;
    assert_eq!(read_all(&browse.volume, &dir.at("nested/b.bin")).await, big_bytes());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_or_a_link_has_no_bytes_to_stream() {
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    for path in [view(), view().at("latest"), dir.at("nested"), dir.at("link")] {
        assert!(
            browse.volume.open_read_stream(&path).await.is_err(),
            "{path:?} has no bytes"
        );
    }
    // Typed, so a copy of the folder skips the link and carries on.
    assert!(matches!(
        browse.volume.open_read_stream(&dir.at("link")).await.err(),
        Some(VolumeError::NoContent(_))
    ));
}

/// A restore's link target: what the backup recorded, unresolved. Nothing else
/// in the view is a link, the view's own folders included.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_reports_the_target_the_backup_recorded() {
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    assert_eq!(
        browse
            .volume
            .read_link_target(&dir.at("link"))
            .await
            .unwrap()
            .as_deref(),
        Some(Path::new("a.txt"))
    );
    for not_a_link in [view(), view().at("latest"), dir.at("nested"), dir.at("a.txt")] {
        assert_eq!(browse.volume.read_link_target(&not_a_link).await.unwrap(), None);
    }
    assert!(matches!(
        browse.volume.read_link_target(&dir.at("nope")).await,
        Err(VolumeError::NotFound(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn ranged_reads_fetch_windows_of_the_pack_never_the_whole_pack() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    let largest_pack = browse
        .fixture
        .files()
        .iter()
        .filter(|(path, _)| path.starts_with("data"))
        .map(|(_, bytes)| bytes.len())
        .max()
        .expect("a backup writes packs");

    let window = browse
        .volume
        .read_range(&dir.at("nested/b.bin"), 1_000, 10)
        .await
        .expect("a ranged read");
    assert_eq!(window, big_bytes()[1_000..1_010]);
    let past_the_end = browse.volume.read_range(&dir.at("a.txt"), 10, 10).await.expect("reads");
    assert!(past_the_end.is_empty(), "a read past the end is empty");

    let reads = browse.parent.read_range_log();
    assert!(
        !reads.is_empty(),
        "file contents come through the parent's `read_range`"
    );
    assert!(
        reads.iter().all(|(_, len)| *len < largest_pack),
        "every read is a window, never a whole pack: {reads:?}"
    );
}

/// ❗ Browsing must never change someone's backup: no write reaches rustic's
/// storage adapter, and the parent's files are byte-identical afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn browsing_and_reading_never_write_to_the_repo() {
    let browse = Browse::new().await;
    let before = browse.fixture.files_at(&root());

    let dir = browse.source_in("latest").await;
    read_all(&browse.volume, &dir.at("nested/b.bin")).await;
    browse.volume.read_range(&dir.at("a.txt"), 0, 5).await.expect("reads");
    browse.volume.scan_for_copy(&view().at("latest")).await.expect("scans");

    assert_eq!(browse.hub.write_attempts(), 0);
    for (path, bytes) in before {
        let on_parent = read_parent(&browse.parent, &path).await;
        assert_eq!(on_parent, bytes, "{path:?} is untouched");
    }
    let locks = browse.parent.list_directory(&root().at("locks"), None).await;
    assert!(locks.expect("locks/").is_empty(), "no restic lock either");
}

async fn read_parent(parent: &InMemoryVolume, path: &AppPath) -> Vec<u8> {
    let mut stream = parent.open_read_stream(path).await.expect("parent file");
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        out.extend_from_slice(&chunk.expect("chunk"));
    }
    out
}

// ── `latest` ───────────────────────────────────────────────────────────────

/// Listing the root only SHOWS `latest`; it reads nothing under it. Someone
/// pressing refresh to see a new backup must see `latest` move.
#[tokio::test(flavor = "multi_thread")]
async fn refreshing_the_root_alone_never_holds_latest_back() {
    let browse = Browse::new().await;
    browse.volume.list_directory(&view(), None).await.expect("lists");

    std::fs::write(browse.source.join("a.txt"), b"second").expect("change the source");
    browse.backup_again(1_792_000_000).await;
    let entries = browse.volume.list_directory(&view(), None).await.expect("refreshes");

    assert_eq!(named(&entries, "latest").modified_at, Some(1_792_000_000));
}

/// ❗ A copy addresses files by path. If a root refresh rebound `latest` in the
/// middle of one, the restore would mix two snapshots, silently. While anything
/// reads under `latest/`, it stays where it was.
#[tokio::test(flavor = "multi_thread")]
async fn latest_doesnt_switch_under_a_running_copy() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    assert_eq!(
        read_all(&browse.volume, &dir.at("a.txt")).await,
        b"hello",
        "the copy's first file"
    );

    std::fs::write(browse.source.join("a.txt"), b"second").expect("change the source");
    browse.backup_again(1_792_000_000).await;
    browse
        .volume
        .list_directory(&view(), None)
        .await
        .expect("another pane refreshes the root");

    assert_eq!(
        read_all(&browse.volume, &dir.at("a.txt")).await,
        b"hello",
        "the rest of the copy still reads the first snapshot"
    );
    let entries = browse.volume.list_directory(&view(), None).await.expect("lists");
    assert_eq!(named(&entries, "latest").modified_at, Some(1_791_000_000));
}

/// Once nothing has read under `latest/` for the quiet period, the next root
/// refresh moves it to the newest snapshot.
#[tokio::test(flavor = "multi_thread")]
async fn latest_moves_on_once_nothing_reads_under_it() {
    let browse = Browse::with_tuning(Tuning {
        latest_quiet: Duration::ZERO,
        ..Tuning::default()
    })
    .await;
    let dir = browse.source_in("latest").await;
    assert_eq!(read_all(&browse.volume, &dir.at("a.txt")).await, b"hello");

    std::fs::write(browse.source.join("a.txt"), b"second").expect("change the source");
    browse.backup_again(1_792_000_000).await;
    browse
        .volume
        .list_directory(&view(), None)
        .await
        .expect("refreshes the root");

    assert_eq!(read_all(&browse.volume, &dir.at("a.txt")).await, b"second");
}

/// ❗ A copy out of `latest/` paused past the quiet period reads nothing (no
/// stream between files, none advancing mid-file), so only its transfer hold
/// says it's still there. While held, a refresh (a person's, or the snapshot
/// watch's) leaves `latest` alone: the rest of the copy must come from the same
/// snapshot. Once the copy settles, the next refresh moves it.
#[tokio::test(flavor = "multi_thread")]
async fn a_paused_copy_out_of_latest_keeps_it_until_the_copy_settles() {
    let browse = Browse::with_tuning(Tuning {
        latest_quiet: Duration::ZERO,
        ..Tuning::default()
    })
    .await;
    let dir = browse.source_in("latest").await;
    let hold = browse.volume.hold_for_transfer().expect("a repo holds for a transfer");
    assert_eq!(read_all(&browse.volume, &dir.at("a.txt")).await, b"hello");

    std::fs::write(browse.source.join("a.txt"), b"second").expect("change the source");
    browse.backup_again(1_792_000_000).await;
    browse
        .volume
        .list_directory(&view(), None)
        .await
        .expect("refreshes the root");
    assert_eq!(
        read_all(&browse.volume, &dir.at("a.txt")).await,
        b"hello",
        "the paused copy's next file still comes from the first snapshot"
    );

    drop(hold);
    browse
        .volume
        .list_directory(&view(), None)
        .await
        .expect("refreshes the root");
    assert_eq!(read_all(&browse.volume, &dir.at("a.txt")).await, b"second");
}

// ── The parent ─────────────────────────────────────────────────────────────

/// An SMB share reconnecting replaces the parent under the same volume id. The
/// hub must read through the new one from then on, ❌ never through the retired
/// one forever; the password survives.
#[tokio::test(flavor = "multi_thread")]
async fn a_replaced_parent_is_read_through_from_then_on() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;

    let replacement = Arc::new(InMemoryVolume::new("testshare"));
    browse.fixture.write_into_at(replacement.as_ref(), &root()).await;
    let reconnected = view_over(&browse.hub, &replacement);
    // The retired parent loses its copy of the repo: anything still reading
    // through it fails from here on.
    for (path, _) in browse.fixture.files_at(&root()) {
        browse.parent.delete(&path).await.expect("rm");
    }

    assert_eq!(reconnected.list_directory(&view(), None).await.expect("lists").len(), 2);
    assert_eq!(read_all(&reconnected, &dir.at("a.txt")).await, b"hello");
}

// ── Trait answers and the shared conformance assertions ───────────────────

#[tokio::test(flavor = "multi_thread")]
async fn it_answers_as_a_read_only_view_routed_over_its_parent() {
    let browse = Browse::new().await;
    let volume = &browse.volume;
    assert_eq!(volume.root().as_str(), format!("{ROOT}/{VIRTUAL_ENTRY_NAME}"));
    assert!(volume.routes_over_a_parent());
    assert!(!volume.is_writable());
    assert!(!volume.can_watch_listings());
    assert!(!volume.supports_local_fs_access());
    assert!(!volume.paths_are_os_visible());
    assert!(volume.local_path().is_none());
    assert!(volume.supports_streaming() && volume.supports_export());
    assert_eq!(volume.lane_key(), browse.parent.lane_key());
    assert_eq!(volume.max_concurrent_scan_listings(), 4);
    assert_eq!(
        volume.backend_kind(),
        BackendKind::Restic,
        "never the `Local` default: favorites, the error report, and the index read it"
    );
    let mkdir = volume.create_directory_all(&view().at("latest/new")).await;
    assert!(matches!(mkdir, Err(VolumeError::NotSupported)), "got {mkdir:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn is_writable_honors_the_shared_declaration_contract() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    conformance::assert_writability_matches_the_mutations_offered(&browse.volume, &std_path_of(&dir.at("scratch")))
        .await;
}

/// A snapshot's rows carry the node's mtime and no other date, as the view's
/// default `supported_dates` claims.
#[tokio::test(flavor = "multi_thread")]
async fn listing_dates_honor_the_shared_claim_contract() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    conformance::assert_listing_dates_are_claimed(&browse.volume, &std_path_of(&dir)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn not_found_honors_the_shared_path_payload_contract() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    conformance::assert_not_found_carries_the_path(&browse.volume, &std_path_of(&dir.at("no-such-file.txt"))).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_scan_stops_when_it_is_told_to() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    conformance::assert_batch_scan_stops_when_told(&browse.volume, &std_path_of(&dir)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_scan_asks_the_boundary_inside_the_walk() {
    let browse = Browse::new().await;
    let dir = browse.source_in("latest").await;
    conformance::assert_batch_scan_asks_inside_the_walk(&browse.volume, &std_path_of(&dir), 2).await;
}
