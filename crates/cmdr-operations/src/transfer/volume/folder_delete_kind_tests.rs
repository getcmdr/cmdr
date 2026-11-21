//! Every engine walk that deletes a folder it walked says so, through
//! `Volume::delete_directory`, ❌ never a plain `delete`.
//!
//! On an object store one name can hold a file and a markerless folder
//! (`notes` beside `notes/…`), and the folder is gone the moment its last
//! child is, so a plain `delete` of the folder's path finds only the file and
//! deletes that: a file nobody selected (S3, issue #510). The backend can
//! refuse that only when the caller tells it a folder is meant.
//!
//! A spy over `InMemoryVolume` records every plain `delete` that lands on a
//! folder, and every `delete_directory`.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError};
use tokio_util::sync::CancellationToken;

use super::cleanup::{TreeRemoval, remove_tree, volume_rollback_with_progress};
use super::{CarriedSource, stamp_source, sweep_carried_source};
use crate::event_sinks::CollectorEventSink;
use crate::ledger::WrittenFile;
use crate::locked_sources::LockedPlan;
use crate::state::WindDown;
use crate::testing::TestOperations;
use crate::types::WriteOperationConfig;
use crate::unblocked_removal::{NodeKind, delete_clearing_blockers};

struct FolderDeleteSpy {
    inner: InMemoryVolume,
    /// Folders a plain `delete` reached.
    blind: Mutex<Vec<PathBuf>>,
    /// What `delete_directory` was asked for.
    as_folders: Mutex<Vec<PathBuf>>,
}

impl FolderDeleteSpy {
    /// `/d/a.txt` and `/d/sub/b.txt`.
    async fn seeded() -> Arc<Self> {
        let inner = InMemoryVolume::new("spy");
        inner.create_directory(app_path!("/d")).await.unwrap();
        inner.create_directory(app_path!("/d/sub")).await.unwrap();
        inner.create_file(app_path!("/d/a.txt"), b"a").await.unwrap();
        inner.create_file(app_path!("/d/sub/b.txt"), b"bb").await.unwrap();
        Arc::new(Self {
            inner,
            blind: Mutex::new(Vec::new()),
            as_folders: Mutex::new(Vec::new()),
        })
    }

    fn blind(&self) -> Vec<PathBuf> {
        self.blind.lock().unwrap().clone()
    }

    fn as_folders(&self) -> Vec<PathBuf> {
        let mut folders = self.as_folders.lock().unwrap().clone();
        folders.sort();
        folders
    }

    /// Both folders went, each as a folder, and nothing deleted one blind.
    async fn assert_folders_deleted_as_folders(&self) {
        assert_eq!(
            self.blind(),
            Vec::<PathBuf>::new(),
            "a folder deleted by a plain `delete`"
        );
        assert_eq!(self.as_folders(), [PathBuf::from("/d"), PathBuf::from("/d/sub")]);
        assert!(!self.inner.exists(app_path!("/d")).await, "the folder went");
    }
}

impl Volume for FolderDeleteSpy {
    crate::forward_volume_methods!(
        inner => name,
        root,
        list_directory,
        get_metadata,
        exists,
        is_directory,
        scan_for_copy,
        scan_for_copy_batch,
    );

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn delete<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let path_app = path;
            let path_std = path.as_std_legacy();
            let path: &Path = &path_std;
            if self.inner.is_directory(path_app).await.unwrap_or(false) {
                self.blind.lock().unwrap().push(path.to_path_buf());
            }
            self.inner.delete(path_app).await
        })
    }

    fn delete_directory<'a>(
        &'a self,
        path: &'a AppPath,
        _cancel: Option<&'a CancellationToken>,
    ) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let path_app = path;
        let path_std = path.as_std_legacy();
        let path: &Path = &path_std;
        self.as_folders.lock().unwrap().push(path.to_path_buf());
        self.inner.delete(path_app)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_delete_walk_deletes_its_folders_as_folders() {
    let spy = FolderDeleteSpy::seeded().await;
    let volume: Arc<dyn Volume> = spy.clone();
    let engine = TestOperations::new();
    engine.volumes().register("spy", volume.clone());
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(0)));

    crate::delete::delete_volume_files_with_progress_inner(
        volume,
        "spy",
        &CollectorEventSink::new(),
        "folder-kind-delete",
        &state,
        &[PathBuf::from("/d")],
        &WriteOperationConfig::default(),
    )
    .await
    .expect("the delete runs");

    spy.assert_folders_deleted_as_folders().await;
}

#[tokio::test]
async fn a_moves_source_sweep_deletes_its_folders_as_folders() {
    let spy = FolderDeleteSpy::seeded().await;
    let volume: Arc<dyn Volume> = spy.clone();
    let carried = stamp_source(&volume, Path::new("/d")).await.expect("the walk stamps");
    assert!(matches!(carried, CarriedSource::Folder(_)));
    let state = Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(0)));

    sweep_carried_source(
        &state,
        &volume,
        Path::new("/d"),
        &carried,
        &Default::default(),
        &LockedPlan::none(),
    )
    .await
    .expect("the sweep runs");

    spy.assert_folders_deleted_as_folders().await;
}

#[tokio::test]
async fn a_tree_removal_deletes_its_folders_as_folders() {
    let spy = FolderDeleteSpy::seeded().await;
    let volume: Arc<dyn Volume> = spy.clone();

    remove_tree(&volume, app_path!("/d"), TreeRemoval::UserChoseOverwriteAcrossTypes)
        .await
        .expect("the tree goes");

    spy.assert_folders_deleted_as_folders().await;
}

/// A cancelled copy's rollback: its files, then the folders it created,
/// deepest first, each pruned once it lists empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copys_rollback_prunes_its_created_folders_as_folders() {
    let spy = FolderDeleteSpy::seeded().await;
    let volume: Arc<dyn Volume> = spy.clone();
    let mut written = vec![
        WrittenFile::volume(PathBuf::from("/d/a.txt"), 1),
        WrittenFile::volume(PathBuf::from("/d/sub/b.txt"), 2),
    ];
    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(0)));

    volume_rollback_with_progress(
        &volume,
        &mut written,
        &[PathBuf::from("/d"), PathBuf::from("/d/sub")],
        &CollectorEventSink::new(),
        "folder-kind-rollback",
        &WindDown::of(&state),
        2,
        3,
        2,
        3,
    )
    .await;

    spy.assert_folders_deleted_as_folders().await;
}

/// The journal rollback's removal of a folder it created (`RemoveDir`) goes
/// through the same unblocking removal, told it's a folder.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unblocking_removal_of_a_folder_deletes_it_as_a_folder() {
    let spy = FolderDeleteSpy::seeded().await;
    let volume: Arc<dyn Volume> = spy.clone();
    volume.delete(app_path!("/d/sub/b.txt")).await.unwrap();

    delete_clearing_blockers(
        &tokio::runtime::Handle::current(),
        &volume,
        Path::new("/d/sub"),
        NodeKind::Directory,
    )
    .await
    .expect("the empty folder goes");

    assert_eq!(
        spy.blind(),
        Vec::<PathBuf>::new(),
        "a folder deleted by a plain `delete`"
    );
    assert_eq!(spy.as_folders(), [PathBuf::from("/d/sub")]);
}
