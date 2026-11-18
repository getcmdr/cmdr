//! The date scenarios every backend owes, written once: a copy keeps the
//! source's modification date, onto the server and off it, for files and for
//! the folders it creates.
//!
//! These run the app's whole pipeline, so they catch what the per-backend
//! conformance cells (`cmdr_fs::volume::conformance`'s two date assertions)
//! can't: a staging rename, a wrapper stream, or an engine path that drops the
//! date between the two ends. The contract:
//! `../transfer/volume/DETAILS.md` § "Copies keep the source's metadata". Cells stay
//! per backend, named for their lane, like `network_transfer_test_support.rs`'s.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::volume::Volume;
use cmdr_fs::volume::conformance::{SOURCE_DATE_SECS, assert_write_from_stream_keeps_the_source_date};

use super::network_transfer_test_support::{clean_deep, run_copy, self_describing_bytes};
use crate::test_support::TestDir;
use cmdr_local::LocalVolume;

/// What `path` lists as its modification date, which the scenarios insist on.
async fn listed_date(volume: &dyn Volume, path: &Path, what: &str) -> u64 {
    volume
        .get_metadata(path)
        .await
        .unwrap_or_else(|e| panic!("{what}: {} must be stattable, got {e:?}", path.display()))
        .modified_at
        .unwrap_or_else(|| panic!("{what}: {} lists no modification date at all", path.display()))
}

/// A file copied onto the server keeps the date it had on local disk, through
/// the app's whole pipeline: the staging name, the final rename, and the
/// checkpoint wrapper around the source stream all sit between the two ends.
///
/// `tolerance` is the server's clock granularity, as for
/// `conformance::assert_write_from_stream_keeps_the_source_date` (which pins
/// the destination half alone, in the backend's own crate).
pub(super) async fn a_copy_onto_the_server_keeps_the_source_date(
    remote: Arc<dyn Volume>,
    dir: PathBuf,
    tolerance: Duration,
) {
    let local_dir = TestDir::new("network_dated_onto_server");
    let local_file = local_dir.join("dated.bin");
    std::fs::write(&local_file, self_describing_bytes(4_000, "dated.bin")).expect("seed the local file");
    let source_date = std::time::UNIX_EPOCH + Duration::from_secs(SOURCE_DATE_SECS);
    std::fs::File::options()
        .write(true)
        .open(&local_file)
        .and_then(|file| file.set_modified(source_date))
        .expect("date the local file into the past");
    let local: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Local",
        &*local_dir,
        crate::volume_host::host(),
    ));

    run_copy(
        "dated-onto-server",
        Arc::clone(&local),
        vec![PathBuf::from("dated.bin")],
        Arc::clone(&remote),
        dir.clone(),
    )
    .await;

    let landed = listed_date(remote.as_ref(), &dir.join("dated.bin"), "after the copy").await;
    let off_by = landed.abs_diff(SOURCE_DATE_SECS);
    assert!(
        off_by <= tolerance.as_secs(),
        "the copy on the server must keep the local file's date ({SOURCE_DATE_SECS}); it lists {landed}, {off_by} s off"
    );

    clean_deep(remote.as_ref(), &dir).await;
}

/// A file copied off the server keeps the date the server lists for it.
///
/// The seed is a dated write through the server's own `write_from_stream`,
/// asserted as it goes, so a server that can't store a date fails at the seed
/// with a sentence saying so, rather than here with a date nobody chose.
pub(super) async fn a_copy_off_the_server_keeps_the_source_date(
    remote: Arc<dyn Volume>,
    dir: PathBuf,
    tolerance: Duration,
) {
    let on_server = dir.join("dated.txt");
    assert_write_from_stream_keeps_the_source_date(remote.as_ref(), &on_server, tolerance).await;
    a_copy_off_the_server_keeps_the_date_it_lists(Arc::clone(&remote), on_server).await;
    clean_deep(remote.as_ref(), &dir).await;
}

/// The date a scenario's copied folder carries: 2021-01-29 08:30:15 UTC.
const FOLDER_DATE_SECS: u64 = SOURCE_DATE_SECS;
/// The folder inside it, dated apart so a stamp on the wrong level can't pass.
const INNER_FOLDER_DATE_SECS: u64 = 1_577_836_800; // 2020-01-01

/// `album/` with a file and `inner/` (holding a file of its own) on local disk
/// under `root`, both folders dated after their contents, the way an old folder
/// lists.
fn seed_dated_local_folder(root: &Path) {
    let album = root.join("album");
    let inner = album.join("inner");
    std::fs::create_dir_all(&inner).expect("seed the local folders");
    std::fs::write(album.join("photo.bin"), self_describing_bytes(3_000, "photo.bin")).expect("seed a file");
    std::fs::write(inner.join("deep.bin"), self_describing_bytes(2_000, "deep.bin")).expect("seed a file");
    date_local_folder(&inner, INNER_FOLDER_DATE_SECS);
    date_local_folder(&album, FOLDER_DATE_SECS);
}

fn date_local_folder(path: &Path, secs: u64) {
    filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(secs as i64, 0))
        .unwrap_or_else(|e| panic!("date {}: {e}", path.display()));
}

fn assert_dated(listed: u64, expected: u64, tolerance: Duration, what: &str) {
    let off_by = listed.abs_diff(expected);
    assert!(
        off_by <= tolerance.as_secs(),
        "{what} must keep its source folder's date ({expected}); it lists {listed}, {off_by} s off"
    );
}

/// A folder copied onto the server keeps its date, and so does the folder
/// inside it, through the app's whole pipeline. Writing each child bumps its
/// folder's date on the server, so this only passes when the copy dates every
/// folder AFTER its contents landed.
///
/// `tolerance` as for [`a_copy_onto_the_server_keeps_the_source_date`].
pub(super) async fn copied_folders_onto_the_server_keep_their_dates(
    remote: Arc<dyn Volume>,
    dir: PathBuf,
    tolerance: Duration,
) {
    let local_dir = TestDir::new("network_dated_folders_onto_server");
    seed_dated_local_folder(&local_dir);
    let local: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Local",
        &*local_dir,
        crate::volume_host::host(),
    ));

    run_copy(
        "dated-folders-onto-server",
        Arc::clone(&local),
        vec![PathBuf::from("album")],
        Arc::clone(&remote),
        dir.clone(),
    )
    .await;

    let album = dir.join("album");
    let listed = listed_date(remote.as_ref(), &album, "the copied folder").await;
    assert_dated(listed, FOLDER_DATE_SECS, tolerance, "the copied folder on the server");
    let listed = listed_date(remote.as_ref(), &album.join("inner"), "the folder inside it").await;
    assert_dated(
        listed,
        INNER_FOLDER_DATE_SECS,
        tolerance,
        "the folder inside it on the server",
    );

    clean_deep(remote.as_ref(), &dir).await;
}

/// A folder copied off the server keeps the date the server lists for it, and
/// so does the folder inside it.
///
/// The seed lands through a plain copy onto the server, then the server's own
/// [`Volume::apply_metadata`] ages both folders (deepest first), so this pins the
/// copy-off half on its own.
pub(super) async fn copied_folders_off_the_server_keep_their_dates(remote: Arc<dyn Volume>, dir: PathBuf) {
    let seed_dir = TestDir::new("network_dated_folders_seed");
    seed_dated_local_folder(&seed_dir);
    let seed: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Seed",
        &*seed_dir,
        crate::volume_host::host(),
    ));
    run_copy(
        "dated-folders-seed",
        seed,
        vec![PathBuf::from("album")],
        Arc::clone(&remote),
        dir.clone(),
    )
    .await;
    let album = dir.join("album");
    let at = |secs: u64| std::time::UNIX_EPOCH + Duration::from_secs(secs);
    for (folder, secs) in [
        (album.join("inner"), INNER_FOLDER_DATE_SECS),
        (album.clone(), FOLDER_DATE_SECS),
    ] {
        let report = remote
            .apply_metadata(&folder, &cmdr_fs::metadata::CarriedMetadata::default(), Some(at(secs)))
            .await;
        assert_eq!(
            report.modified,
            cmdr_fs::metadata::ClassOutcome::Kept,
            "fixture: date {} on the server, got {report:?}",
            folder.display()
        );
    }
    let listed_album = listed_date(remote.as_ref(), &album, "the seed").await;
    let listed_inner = listed_date(remote.as_ref(), &album.join("inner"), "the seed").await;

    let local_dir = TestDir::new("network_dated_folders_off_server");
    let local: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Local",
        &*local_dir,
        crate::volume_host::host(),
    ));
    run_copy(
        "dated-folders-off-server",
        Arc::clone(&remote),
        vec![album],
        Arc::clone(&local),
        PathBuf::from(""),
    )
    .await;

    // Local disk keeps nanoseconds, so only the server's rounding can put the
    // two a second apart.
    let landed = listed_date(local.as_ref(), Path::new("album"), "after the copy").await;
    assert_dated(
        landed,
        listed_album,
        Duration::from_secs(1),
        "the copied folder on local disk",
    );
    let landed = listed_date(local.as_ref(), Path::new("album/inner"), "after the copy").await;
    assert_dated(
        landed,
        listed_inner,
        Duration::from_secs(1),
        "the folder inside it on local disk",
    );

    clean_deep(remote.as_ref(), &dir).await;
}

/// A file already on the server, listed with a date at least a day old, keeps
/// that date when copied off it.
///
/// For a server that can't store a date (Apache `mod_dav`), whose fixture dates
/// a file by its own means; [`a_copy_off_the_server_keeps_the_source_date`]
/// seeds one through the server for everything else. Leaves `on_server` alone.
pub(super) async fn a_copy_off_the_server_keeps_the_date_it_lists(remote: Arc<dyn Volume>, on_server: PathBuf) {
    let source_date = listed_date(remote.as_ref(), &on_server, "the seed").await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is past 1970")
        .as_secs();
    assert!(
        source_date + 24 * 60 * 60 <= now,
        "fixture precondition: {} must list a date at least a day old, so a copy stamping \"now\" can't pass; it lists {source_date}",
        on_server.display()
    );
    let name = on_server
        .file_name()
        .expect("the seed is a file, so it has a name")
        .to_owned();

    let local_dir = TestDir::new("network_dated_off_server");
    let local: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Local",
        &*local_dir,
        crate::volume_host::host(),
    ));
    run_copy(
        "dated-off-server",
        Arc::clone(&remote),
        vec![on_server],
        Arc::clone(&local),
        PathBuf::from(""),
    )
    .await;

    // Local disk keeps nanoseconds, so only the server's own rounding of the
    // date it reports can put the two a second apart.
    let landed = listed_date(local.as_ref(), Path::new(&name), "after the copy").await;
    let off_by = landed.abs_diff(source_date);
    assert!(
        off_by <= 1,
        "the copy on local disk must keep the date the server lists ({source_date}); it lists {landed}, {off_by} s off"
    );
}

/// What `path` lists as its creation date.
#[cfg(target_os = "macos")]
async fn listed_creation_date(volume: &dyn Volume, path: &Path, what: &str) -> u64 {
    volume
        .get_metadata(path)
        .await
        .unwrap_or_else(|e| panic!("{what}: {} must be stattable, got {e:?}", path.display()))
        .created_at
        .unwrap_or_else(|| panic!("{what}: {} lists no creation date at all", path.display()))
}

/// Bigger than one write frame on every fixture, so a copy of it streams
/// through the writer instead of the one-frame compound write.
#[cfg(target_os = "macos")]
const STREAMED_BYTES: usize = 10 * 1024 * 1024;

/// Files and a folder copied onto the server from local disk, each born years
/// before it was last modified, keep their creation dates: a small file (one
/// frame), a big one (the streaming writer), and a folder with the file inside.
///
/// For a server that stores creation dates (SMB's `CreationTime`).
#[cfg(target_os = "macos")]
pub(super) async fn a_copy_onto_the_server_keeps_the_creation_dates(remote: Arc<dyn Volume>, dir: PathBuf) {
    use cmdr_fs::testing::metadata_fixture::{
        ACCESSED_SECS, BIRTH_SECS, Follow, MODIFIED_SECS, MetadataFixture, birth_secs, set_times,
    };

    let fixture = MetadataFixture::new("network_birth_onto_server");
    let small = fixture.rich_file("born.bin");
    let big = fixture.join("big.bin");
    std::fs::write(&big, self_describing_bytes(STREAMED_BYTES, "big.bin")).expect("seed the big file");
    set_times(&big, BIRTH_SECS, MODIFIED_SECS, ACCESSED_SECS, Follow::Yes);
    let album = fixture.join("album");
    std::fs::create_dir(&album).expect("seed the folder");
    std::fs::write(album.join("inside.bin"), b"inside").expect("seed the folder's file");
    set_times(&album, BIRTH_SECS, MODIFIED_SECS, ACCESSED_SECS, Follow::Yes);
    for seeded in [&small, &big, &album] {
        assert_eq!(
            birth_secs(seeded),
            BIRTH_SECS,
            "fixture precondition: {}",
            seeded.display()
        );
    }
    assert!(
        !remote
            .write_is_single_shot(cmdr_fs::volume::StreamLength::Known(STREAMED_BYTES as u64))
            .await,
        "fixture precondition: the big file must stream, not go out in one frame"
    );
    let local: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Local",
        &*fixture,
        crate::volume_host::host(),
    ));

    run_copy(
        "birth-onto-server",
        Arc::clone(&local),
        vec![
            PathBuf::from("born.bin"),
            PathBuf::from("big.bin"),
            PathBuf::from("album"),
        ],
        Arc::clone(&remote),
        dir.clone(),
    )
    .await;

    for name in ["born.bin", "big.bin", "album"] {
        let landed = dir.join(name);
        let created = listed_creation_date(remote.as_ref(), &landed, "after the copy").await;
        assert_eq!(
            created, BIRTH_SECS as u64,
            "{name} on the server must be born when the local one was (2015-03-01)"
        );
        let modified = listed_date(remote.as_ref(), &landed, "after the copy").await;
        assert_eq!(
            modified, MODIFIED_SECS as u64,
            "{name}: the modification date must stay too"
        );
    }

    clean_deep(remote.as_ref(), &dir).await;
}

/// Files and a folder copied off the server keep the creation dates the server
/// lists: a small file (the one-frame read), a big one (the streamed download),
/// and a folder. Each is listed with a modification date years before its
/// creation date, so a copy that drops birth reads that modification date on
/// APFS (it drags birth down to an earlier mtime) and fails.
#[cfg(target_os = "macos")]
pub(super) async fn a_copy_off_the_server_keeps_the_creation_dates(remote: Arc<dyn Volume>, dir: PathBuf) {
    use cmdr_fs::metadata::CarriedMetadata;
    use cmdr_fs::testing::metadata_fixture::birth_secs;

    let small = dir.join("born.txt");
    assert_write_from_stream_keeps_the_source_date(remote.as_ref(), &small, Duration::ZERO).await;
    let big = dir.join("big.bin");
    remote
        .create_file(&big, &self_describing_bytes(STREAMED_BYTES, "big.bin"))
        .await
        .expect("seed the big file on the server");
    let album = dir.join("album");
    remote
        .create_directory(&album)
        .await
        .expect("seed the folder on the server");
    remote
        .create_file(&album.join("inside.txt"), b"inside")
        .await
        .expect("seed the folder's file");
    let aged = std::time::UNIX_EPOCH + Duration::from_secs(SOURCE_DATE_SECS);
    for seeded in [&big, &album] {
        let report = remote
            .apply_metadata(seeded, &CarriedMetadata::default(), Some(aged))
            .await;
        assert_eq!(
            report.modified,
            cmdr_fs::metadata::ClassOutcome::Kept,
            "fixture: age {}",
            seeded.display()
        );
    }
    let mut listed = Vec::new();
    for seeded in [&small, &big, &album] {
        let created = listed_creation_date(remote.as_ref(), seeded, "the seed").await;
        let modified = listed_date(remote.as_ref(), seeded, "the seed").await;
        assert!(
            created > modified + 24 * 60 * 60,
            "fixture precondition: {} must be created well after its modification date; it lists created {created}, modified {modified}",
            seeded.display()
        );
        listed.push(created);
    }

    let local_dir = TestDir::new("network_birth_off_server");
    let local: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Local",
        &*local_dir,
        crate::volume_host::host(),
    ));
    run_copy(
        "birth-off-server",
        Arc::clone(&remote),
        vec![small, big, album],
        Arc::clone(&local),
        PathBuf::from(""),
    )
    .await;

    for (name, created) in ["born.txt", "big.bin", "album"].into_iter().zip(listed) {
        let landed_birth = birth_secs(&local_dir.join(name)).unsigned_abs();
        assert_eq!(
            landed_birth, created,
            "{name} on local disk must be born when the server says it was"
        );
    }

    clean_deep(remote.as_ref(), &dir).await;
}

/// A file and a folder copied from the server onto the same server keep the
/// creation dates it lists: the copy streams through the app, so the date has
/// to ride the stream and land on the copy like any other.
#[cfg(target_os = "macos")]
pub(super) async fn a_copy_within_the_server_keeps_the_creation_dates(remote: Arc<dyn Volume>, dir: PathBuf) {
    use cmdr_fs::metadata::{CarriedMetadata, ClassOutcome};
    use cmdr_fs::volume::conformance::BIRTH_SECS;

    let file = dir.join("born.txt");
    remote.create_file(&file, b"born in 2015").await.expect("seed the file");
    let album = dir.join("album");
    remote.create_directory(&album).await.expect("seed the folder");
    remote
        .create_file(&album.join("inside.txt"), b"inside")
        .await
        .expect("seed the folder's file");
    let born = CarriedMetadata {
        created: Some(std::time::UNIX_EPOCH + Duration::from_secs(BIRTH_SECS)),
        ..CarriedMetadata::default()
    };
    let aged = Some(std::time::UNIX_EPOCH + Duration::from_secs(SOURCE_DATE_SECS));
    for seeded in [&file, &album] {
        let report = remote.apply_metadata(seeded, &born, aged).await;
        assert_eq!(
            report.created,
            ClassOutcome::Kept,
            "fixture: birth on {}",
            seeded.display()
        );
    }
    let copies = dir.join("copies");
    remote
        .create_directory(&copies)
        .await
        .expect("make the destination folder");

    run_copy(
        "birth-within-server",
        Arc::clone(&remote),
        vec![file, album],
        Arc::clone(&remote),
        copies.clone(),
    )
    .await;

    for name in ["born.txt", "album"] {
        let created = listed_creation_date(remote.as_ref(), &copies.join(name), "after the copy").await;
        assert_eq!(
            created, BIRTH_SECS,
            "the copy of {name} must be born when the original was"
        );
    }

    clean_deep(remote.as_ref(), &dir).await;
}
