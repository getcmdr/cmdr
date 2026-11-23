//! A cross-volume copy or move that leaves the running OS's junk files out
//! (#314, #1305), over `InMemoryVolume`, the double every backend's copy walk is
//! held to: the merge walk never lands one, and a move's sweep takes them with
//! the folders that moved.

use super::test_support::make_volumes;
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use cmdr_fs::app_path;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::os_junk_files::{JunkOs, OsJunkFiles};
use std::time::Duration;

/// The running OS's junk names, so the test checks the set this OS skips.
const JUNK: [&str; 2] = JunkOs::current().sample_junk_names();

/// `dir`'s junk file number `n` as an app path.
fn junk_in(dir: &str, n: usize) -> app_path::AppPathBuf {
    app_path::AppPathBuf::parse(&format!("{dir}/{}", JUNK[n]), app_path::PathFlavor::Posix).expect("a canonical path")
}

async fn seed(source: &Arc<dyn Volume>) {
    source.create_directory(app_path!("/work")).await.unwrap();
    source.create_directory(app_path!("/work/notes")).await.unwrap();
    source.create_directory(app_path!("/work/only-metadata")).await.unwrap();
    source.create_file(&junk_in("/work", 0), &[0u8; 64]).await.unwrap();
    source.create_file(&junk_in("/work", 1), &[0u8; 32]).await.unwrap();
    source.create_file(app_path!("/work/report.txt"), b"one").await.unwrap();
    source
        .create_file(&junk_in("/work/notes", 0), &[0u8; 64])
        .await
        .unwrap();
    source
        .create_file(app_path!("/work/notes/todo.md"), b"two")
        .await
        .unwrap();
    source
        .create_file(&junk_in("/work/only-metadata", 0), &[0u8; 64])
        .await
        .unwrap();
}

fn skipping_state(engine: &TestOperations) -> Arc<WriteOperationState> {
    Arc::new(
        engine
            .new_operation_state(Duration::from_millis(0))
            .leaving_out(OsJunkFiles::Skip),
    )
}

fn skipping_config() -> VolumeCopyConfig {
    VolumeCopyConfig {
        os_junk_files: OsJunkFiles::Skip,
        ..VolumeCopyConfig::default()
    }
}

async fn assert_lands_without_os_junk_files(dest: &Arc<dyn Volume>) {
    assert!(dest.exists(app_path!("/work/report.txt")).await);
    assert!(dest.exists(app_path!("/work/notes/todo.md")).await);
    assert!(
        dest.exists(app_path!("/work/only-metadata")).await,
        "the folder is the person's"
    );
    for gone in [
        junk_in("/work", 0),
        junk_in("/work", 1),
        junk_in("/work/notes", 0),
        junk_in("/work/only-metadata", 0),
    ] {
        assert!(!dest.exists(&gone).await, "{gone:?} reached the destination");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_volume_copy_that_skips_os_junk_files_writes_and_counts_none_of_them() {
    let engine = TestOperations::new();
    let (source, dest) = make_volumes();
    seed(&source).await;

    let events = Arc::new(CollectorEventSink::new());
    super::super::copy_volumes_with_progress(
        events.clone(),
        "op-volume-copy-skip-mac",
        &skipping_state(&engine),
        Arc::clone(&source),
        &[PathBuf::from("/work")],
        Arc::clone(&dest),
        Path::new("/"),
        &skipping_config(),
    )
    .await
    .expect("the copy");

    assert_lands_without_os_junk_files(&dest).await;
    let bytes_processed = events.complete.lock_ignore_poison()[0].bytes_processed;
    assert_eq!(bytes_processed, 6, "the totals leave them out too");
    assert!(
        source.exists(&junk_in("/work", 0)).await,
        "a copy never touches the source"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_volume_move_that_skips_os_junk_files_leaves_no_husk_in_the_source() {
    let engine = TestOperations::new();
    let (source, dest) = make_volumes();
    seed(&source).await;

    let events = Arc::new(CollectorEventSink::new());
    move_volumes_with_progress(
        events.clone(),
        "op-volume-move-skip-mac",
        &skipping_state(&engine),
        Arc::clone(&source),
        &[PathBuf::from("/work")],
        Arc::clone(&dest),
        Path::new("/"),
        &skipping_config(),
    )
    .await
    .expect("the move");

    assert_lands_without_os_junk_files(&dest).await;
    assert!(
        !source.exists(app_path!("/work")).await,
        "the folders moved, so their metadata went with them"
    );
}

/// A folder that stays for a real reason keeps its metadata.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_volume_move_keeps_the_metadata_of_a_folder_that_stays() {
    let engine = TestOperations::new();
    let (source, dest) = make_volumes();
    seed(&source).await;
    // Already at the destination, and Skip keeps the source's copy, so its
    // folder stays.
    dest.create_directory(app_path!("/work")).await.unwrap();
    dest.create_directory(app_path!("/work/notes")).await.unwrap();
    dest.create_file(app_path!("/work/notes/todo.md"), b"old")
        .await
        .unwrap();

    let events = Arc::new(CollectorEventSink::new());
    let config = VolumeCopyConfig {
        conflict_resolution: crate::types::ConflictResolution::Skip,
        ..skipping_config()
    };
    move_volumes_with_progress(
        events.clone(),
        "op-volume-move-kept",
        &skipping_state(&engine),
        Arc::clone(&source),
        &[PathBuf::from("/work")],
        Arc::clone(&dest),
        Path::new("/"),
        &config,
    )
    .await
    .expect("the move");

    assert!(
        source.exists(app_path!("/work/notes/todo.md")).await,
        "the Skip kept it"
    );
    assert!(
        source.exists(&junk_in("/work/notes", 0)).await,
        "so its folder keeps its metadata"
    );
    assert!(
        !source.exists(app_path!("/work/only-metadata")).await,
        "a folder that went took its metadata along"
    );
}

/// A junk file the source won't let go of (Explorer holding `Thumbs.db`, an open
/// `.nfs*` on Linux, a share's sharing violation) keeps its folder, and the move
/// still succeeds: it's junk the person asked to skip, ❌ never a failure (#1305).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_junk_file_the_source_holds_keeps_its_folder_and_the_move_succeeds() {
    use super::super::faulty_volume::{FaultyOp, FaultyVolume};
    use cmdr_fs::volume::{InMemoryVolume, VolumeError};

    let held = junk_in("/work/only-metadata", 0);
    for refusal in [
        VolumeError::FileInUse(held.as_str().to_owned()),
        VolumeError::PermissionDenied {
            path: held.as_str().to_owned(),
            raw_os_error: None,
        },
    ] {
        let engine = TestOperations::new();
        let inner = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
        seed(&(Arc::clone(&inner) as Arc<dyn Volume>)).await;
        let refusing = FaultyVolume::wrapping(Arc::clone(&inner))
            .failing_call_at(FaultyOp::Delete, held.as_str(), 1, refusal)
            .arc();
        let (_, dest) = make_volumes();

        let events = Arc::new(CollectorEventSink::new());
        move_volumes_with_progress(
            events.clone(),
            "op-volume-move-held-junk",
            &skipping_state(&engine),
            Arc::clone(&refusing) as Arc<dyn Volume>,
            &[PathBuf::from("/work")],
            Arc::clone(&dest),
            Path::new("/"),
            &skipping_config(),
        )
        .await
        .expect("a held junk file is no failure");

        assert!(refusing.fault_fired(FaultyOp::Delete), "the refusal reached the sweep");
        assert_lands_without_os_junk_files(&dest).await;
        assert!(inner.exists(&held).await, "the held file stays");
        assert!(
            !inner.exists(app_path!("/work/notes")).await,
            "every other folder still went"
        );
    }
}
