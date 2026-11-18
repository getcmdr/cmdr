//! A cross-volume copy onto local disk keeps what its source carries beyond the
//! bytes: the creation dates of files AND folders, and a routed source's mode,
//! folded through the umask.
//!
//! The source is `InMemoryVolume`, the reference that reports every class, so
//! these pin the engine and the local destination between the two ends: the
//! stream's `metadata`, the staged write, the landing rename, and the folder
//! pass. The contract: `DETAILS.md` § "Copies keep the source's metadata".
#![cfg(unix)] // POSIX-only cells: `LocalPosixVolume`, POSIX modes, or errnos (windows-port plan, tasks 35 and 36).

use cmdr_fs::app_path::AppPath;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::metadata::CarriedMetadata;

use super::copy_volumes_with_progress;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::VolumeCopyConfig;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_local::LocalPosixVolume;

async fn copy_onto_local(
    engine: &TestOperations,
    op: &str,
    source: Arc<InMemoryVolume>,
    paths: &[PathBuf],
    dest_dir: &Path,
) {
    let dest: Arc<dyn Volume> = Arc::new(LocalPosixVolume::local_folder(
        "Local",
        dest_dir,
        crate::host::testing::pane_order_host(),
    ));
    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        op,
        &Arc::new(engine.new_operation_state(Duration::from_millis(50))),
        source as Arc<dyn Volume>,
        paths,
        dest,
        Path::new(""),
        &VolumeCopyConfig::default(),
    )
    .await
    .unwrap_or_else(|failure| panic!("{op}: the copy succeeds, got {:?}", failure.error));
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("{} must exist: {e}", path.display()))
        .permissions()
        .mode()
        & 0o7777
}

/// A routed source's mode rides its stream, so a file selected on its own lands
/// with it though nothing stat'ed the source for it (`InMemoryVolume` doesn't
/// even claim `reports_posix_mode`). Folded through the umask: an executable
/// stays one, a private file stays private, and a source with no mode lands at
/// the umask's default.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_mode_lands_folded_with_no_source_stat() {
    let engine = TestOperations::new();
    let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    for (name, mode) in [("run.sh", 0o755), ("private.txt", 0o600), ("modeless.txt", 0)] {
        let path = PathBuf::from("/").join(name);
        source
            .create_file(&AppPath::legacy_unchecked(&path), b"#!/bin/sh\n")
            .await
            .unwrap();
        let carried = CarriedMetadata {
            mode: Some(mode),
            ..CarriedMetadata::default()
        };
        source.set_carried_metadata(&path, carried);
    }
    let dest_dir = TestDir::new("copy_metadata_mode");
    let default = cmdr_fs::metadata::default_file_mode();

    copy_onto_local(
        &engine,
        "op-routed-mode",
        source,
        &[
            PathBuf::from("/run.sh"),
            PathBuf::from("/private.txt"),
            PathBuf::from("/modeless.txt"),
        ],
        &dest_dir,
    )
    .await;

    assert_eq!(
        mode_of(&dest_dir.join("run.sh")),
        cmdr_fs::metadata::routed_mode(0o755, default).expect("a mode"),
        "an executable lands executable"
    );
    assert_eq!(
        mode_of(&dest_dir.join("private.txt")),
        0o600,
        "a private file stays private"
    );
    assert_eq!(
        mode_of(&dest_dir.join("modeless.txt")),
        default,
        "a source with no mode lands at the umask's default"
    );
}

/// Birth is macOS-only: Linux can't set one.
#[cfg(target_os = "macos")]
mod birth {
    use cmdr_fs::app_path;
    use cmdr_fs::app_path::AppPath;
    use std::time::UNIX_EPOCH;

    use super::*;

    /// The top-level folder's creation date: 2015-03-01.
    const ALBUM_BORN: u64 = 1_425_168_000;
    /// The folder inside it, born apart so a stamp on the wrong level can't pass.
    const INNER_BORN: u64 = 1_451_606_400; // 2016-01-01
    /// A file's: 2017-06-01.
    const PHOTO_BORN: u64 = 1_496_275_200;
    /// Every item's modification date, years after each birth (2021-01-29), so a
    /// destination that sets only mtime shows birth == mtime on APFS and fails.
    const MODIFIED: u64 = 1_611_909_015;

    fn at(secs: u64) -> std::time::SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn born(secs: u64) -> CarriedMetadata {
        CarriedMetadata {
            created: Some(at(secs)),
            ..CarriedMetadata::default()
        }
    }

    /// `/album` holding `photo.bin` and `inner/deep.bin`, each born on its own date
    /// and modified years later, the way a folder of old photos lists.
    async fn seed_born_album(source: &InMemoryVolume) -> PathBuf {
        let album = PathBuf::from("/album");
        let inner = album.join("inner");
        source
            .create_directory(&AppPath::legacy_unchecked(&album))
            .await
            .unwrap();
        source
            .create_directory(&AppPath::legacy_unchecked(&inner))
            .await
            .unwrap();
        source
            .create_file(&AppPath::legacy_unchecked(&album.join("photo.bin")), b"photo")
            .await
            .unwrap();
        source
            .create_file(&AppPath::legacy_unchecked(&inner.join("deep.bin")), b"deep")
            .await
            .unwrap();
        source.set_carried_metadata(&album.join("photo.bin"), born(PHOTO_BORN));
        source.set_carried_metadata(&inner.join("deep.bin"), born(PHOTO_BORN));
        source.set_carried_metadata(&inner, born(INNER_BORN));
        source.set_carried_metadata(&album, born(ALBUM_BORN));
        for path in [
            album.join("photo.bin"),
            inner.join("deep.bin"),
            inner.clone(),
            album.clone(),
        ] {
            source.set_modified_at(&path, Some(MODIFIED));
        }
        album
    }

    fn birth_of(path: &Path) -> u64 {
        std::fs::metadata(path)
            .and_then(|meta| meta.created())
            .unwrap_or_else(|e| panic!("{} must have a birth time: {e}", path.display()))
            .duration_since(UNIX_EPOCH)
            .expect("after 1970")
            .as_secs()
    }

    /// Lars's ask: a folder of old files copied onto local disk keeps every
    /// creation date, on the files and on the folders, beside the modification
    /// dates.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_copy_onto_local_disk_keeps_the_creation_dates_of_files_and_folders() {
        let engine = TestOperations::new();
        let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
        let album = seed_born_album(&source).await;
        let dest_dir = TestDir::new("copy_metadata_birth");

        copy_onto_local(&engine, "op-birth-onto-local", source, &[album], &dest_dir).await;

        let landed = dest_dir.join("album");
        assert_eq!(
            birth_of(&landed.join("photo.bin")),
            PHOTO_BORN,
            "a file keeps its birth"
        );
        assert_eq!(birth_of(&landed.join("inner/deep.bin")), PHOTO_BORN, "a deep file too");
        assert_eq!(
            birth_of(&landed.join("inner")),
            INNER_BORN,
            "the folder inside keeps its birth"
        );
        assert_eq!(birth_of(&landed), ALBUM_BORN, "the top-level folder keeps its birth");
        let modified = std::fs::metadata(landed.join("photo.bin"))
            .and_then(|meta| meta.modified())
            .expect("mtime");
        assert_eq!(modified, at(MODIFIED), "and the modification date stays");
    }

    /// A file selected on its own goes through the top-level dispatch, which has
    /// no listing in hand: its birth still rides the stream.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_file_selected_on_its_own_keeps_its_creation_date() {
        let engine = TestOperations::new();
        let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
        source.create_file(app_path!("/old.txt"), b"old").await.unwrap();
        source.set_carried_metadata(Path::new("/old.txt"), born(PHOTO_BORN));
        source.set_modified_at(Path::new("/old.txt"), Some(MODIFIED));
        let dest_dir = TestDir::new("copy_metadata_birth_alone");

        copy_onto_local(
            &engine,
            "op-birth-alone",
            source,
            &[PathBuf::from("/old.txt")],
            &dest_dir,
        )
        .await;

        assert_eq!(birth_of(&dest_dir.join("old.txt")), PHOTO_BORN);
    }
}
