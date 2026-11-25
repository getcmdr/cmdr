//! A cross-volume copy keeps Hidden and Locked on files and folders, putting
//! Locked on only once each item has its final name (`landed_flags.rs`), and
//! spending a call after the landing only on a file that needs one.
//!
//! Both ends are `InMemoryVolume`, the reference: one with every class, one
//! configured to SMB's caps (Hidden rides the write, Read-only waits for the
//! landing). The real share's half is `cmdr-smb`'s
//! `attributes_integration_test.rs` and the app's `smb_transfer_semantics_test.rs`
//! cells. The contract: `DETAILS.md` § "Copies keep the source's metadata".

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cmdr_fs::metadata::{CarriedMetadata, MetadataReport, UserFlags};

use super::copy_volumes_with_progress;
use super::faulty_volume::forward_volume_methods;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, VolumeCopyConfig};
use cmdr_fs::volume::{InMemoryVolume, Volume};

/// A destination that counts the `apply_metadata` calls on FILES, the one a
/// copy spends after a landing.
struct CountingDest {
    inner: Arc<InMemoryVolume>,
    file_applies: AtomicUsize,
}

impl Volume for CountingDest {
    forward_volume_methods!(inner =>
        name, root, list_directory, get_metadata, exists, is_directory, create_file, create_directory,
        create_directory_all, delete, rename, get_space_info, supports_streaming, supports_export,
        operations_are_local, max_concurrent_ops, scan_for_copy, supports_unknown_length_writes,
        write_from_stream, open_read_stream, metadata_caps, folder_metadata,
    );

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn apply_metadata<'a>(
        &'a self,
        path: &'a AppPath,
        carried: &'a CarriedMetadata,
        modified: Option<std::time::SystemTime>,
    ) -> Pin<Box<dyn Future<Output = MetadataReport> + Send + 'a>> {
        Box::pin(async move {
            if !self.inner.is_directory(path).await.unwrap_or(false) {
                self.file_applies.fetch_add(1, Ordering::SeqCst);
            }
            self.inner.apply_metadata(path, carried, modified).await
        })
    }
}

async fn copy(
    engine: &TestOperations,
    source: Arc<InMemoryVolume>,
    paths: &[&str],
    dest: Arc<dyn Volume>,
    config: &VolumeCopyConfig,
) {
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-copy-flags",
        &Arc::new(engine.new_operation_state(Duration::from_millis(50))),
        source as Arc<dyn Volume>,
        &crate::testing::app_paths_of(&paths),
        dest,
        &crate::testing::app_path_of(Path::new("/")),
        config,
    )
    .await
    .unwrap_or_else(|failure| panic!("the copy succeeds, got {:?}", failure.error));
}

async fn seed_file(volume: &InMemoryVolume, path: &str, flags: UserFlags) {
    volume
        .create_file(AppPath::from_normalized(path).expect("a canonical path"), b"bytes")
        .await
        .unwrap();
    volume.set_carried_metadata(
        Path::new(path),
        CarriedMetadata {
            flags,
            ..CarriedMetadata::default()
        },
    );
}

fn source() -> Arc<InMemoryVolume> {
    Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000))
}

fn flags_at(volume: &InMemoryVolume, path: &str) -> UserFlags {
    volume
        .carried_metadata(Path::new(path))
        .unwrap_or_else(|| panic!("{path} must have landed"))
        .flags
}

/// Files and a folder land wearing their source's Hidden and Locked: a file
/// once it has its final name, a folder once its contents are in.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hidden_and_locked_land_on_files_and_folders() {
    let engine = TestOperations::new();
    let source = source();
    seed_file(&source, "/locked.txt", UserFlags::HIDDEN | UserFlags::LOCKED).await;
    seed_file(&source, "/hidden.txt", UserFlags::HIDDEN).await;
    source.create_directory(app_path!("/album")).await.unwrap();
    seed_file(&source, "/album/inside.txt", UserFlags::NONE).await;
    source.set_carried_metadata(
        Path::new("/album"),
        CarriedMetadata {
            flags: UserFlags::LOCKED,
            ..CarriedMetadata::default()
        },
    );
    let dest = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));

    copy(
        &engine,
        source,
        &["/locked.txt", "/hidden.txt", "/album"],
        dest.clone() as Arc<dyn Volume>,
        &VolumeCopyConfig::default(),
    )
    .await;

    assert_eq!(flags_at(&dest, "/locked.txt"), UserFlags::HIDDEN | UserFlags::LOCKED);
    assert_eq!(flags_at(&dest, "/hidden.txt"), UserFlags::HIDDEN);
    assert_eq!(flags_at(&dest, "/album"), UserFlags::LOCKED, "a folder keeps its flags");
    assert!(dest.exists(app_path!("/album/inside.txt")).await);
}

/// An Overwrite lands through a safe-replace swap, so the file takes its flags
/// after it, on the ORIGINAL's name, never on the temp.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_overwrite_lands_locked_at_the_name_it_replaced() {
    let engine = TestOperations::new();
    let source = source();
    seed_file(&source, "/notes.txt", UserFlags::LOCKED).await;
    let dest = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    dest.create_file(app_path!("/notes.txt"), b"the old notes")
        .await
        .unwrap();

    copy(
        &engine,
        source,
        &["/notes.txt"],
        dest.clone() as Arc<dyn Volume>,
        &VolumeCopyConfig {
            conflict_resolution: ConflictResolution::Overwrite,
            ..VolumeCopyConfig::default()
        },
    )
    .await;

    assert_eq!(flags_at(&dest, "/notes.txt"), UserFlags::LOCKED);
    let names: Vec<String> = dest
        .list_directory(app_path!("/"), None)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, vec!["notes.txt".to_string()], "no temp left behind");
}

/// Onto SMB's caps, Hidden rides the write and costs no call after the
/// landing; Locked costs one, and it carries the whole set so the replacing
/// attribute write keeps Hidden. A plain file costs nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn onto_smbs_caps_only_a_locked_file_costs_a_call_after_its_landing() {
    let engine = TestOperations::new();
    let source = source();
    seed_file(&source, "/plain.txt", UserFlags::NONE).await;
    seed_file(&source, "/hidden.txt", UserFlags::HIDDEN).await;
    let inner = Arc::new(
        InMemoryVolume::new("Share")
            .with_space_info(10_000_000, 10_000_000)
            .with_metadata_caps(cmdr_smb::volume::SmbVolume::METADATA_CAPS),
    );
    let dest = Arc::new(CountingDest {
        inner: Arc::clone(&inner),
        file_applies: AtomicUsize::new(0),
    });

    copy(
        &engine,
        Arc::clone(&source),
        &["/plain.txt", "/hidden.txt"],
        dest.clone() as Arc<dyn Volume>,
        &VolumeCopyConfig::default(),
    )
    .await;
    assert_eq!(
        dest.file_applies.load(Ordering::SeqCst),
        0,
        "nothing waits for the landing"
    );
    assert_eq!(
        flags_at(&inner, "/hidden.txt"),
        UserFlags::HIDDEN,
        "Hidden rode the write"
    );

    seed_file(&source, "/locked.txt", UserFlags::HIDDEN | UserFlags::LOCKED).await;
    copy(
        &engine,
        source,
        &["/locked.txt"],
        dest.clone() as Arc<dyn Volume>,
        &VolumeCopyConfig::default(),
    )
    .await;
    assert_eq!(dest.file_applies.load(Ordering::SeqCst), 1, "Locked costs one call");
    assert_eq!(flags_at(&inner, "/locked.txt"), UserFlags::HIDDEN | UserFlags::LOCKED);
}
