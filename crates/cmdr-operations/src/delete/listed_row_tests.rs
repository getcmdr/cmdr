//! The volume delete's scan reads each child through `listed_path`: a `..` row stops the
//! delete before anything goes, and a name holding `\` (APFS's, SFTP's, or MTP's one-name
//! `foo\..`) is a plain name on every host's POSIX volume. An absolute row outside the
//! listed folder still passes, as it always did (#1444).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::super::event_sinks::CollectorEventSink;
use super::super::state::WriteOperationState;
use super::super::testing::TestOperations;
use super::super::types::WriteOperationConfig;
use super::volume_walker::delete_volume_files_with_progress_inner;
use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{InMemoryVolume, ListingProgress, Volume, VolumeError};

/// An in-memory volume whose listing of `/album` reports `/album/../victim.txt` as the path
/// of the row really at `/album/evil.txt`.
struct ClimbingRow {
    inner: InMemoryVolume,
}

impl Volume for ClimbingRow {
    crate::forward_volume_methods!(inner => name, root, get_metadata, exists, is_directory, create_directory,
        delete, get_space_info);
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = self.inner.list_directory(path, on_progress).await?;
            for entry in &mut entries {
                if entry.path == "/album/evil.txt" {
                    entry.path = "/album/../victim.txt".to_string();
                }
            }
            Ok(entries)
        })
    }
}

fn make_state(engine: &TestOperations) -> Arc<WriteOperationState> {
    Arc::new(engine.new_operation_state(Duration::from_millis(0)))
}

async fn delete(engine: &TestOperations, volume: Arc<dyn Volume>, source: &str) -> Result<(), String> {
    delete_volume_files_with_progress_inner(
        volume,
        "mem",
        &CollectorEventSink::new(),
        "op-delete-listed-row",
        &make_state(engine),
        &[PathBuf::from(source)],
        &WriteOperationConfig::default(),
    )
    .await
    .map_err(|e| format!("{e:?}"))
}

/// A climbing row stops the delete before anything goes: neither the place it names nor
/// the folder it was listed in.
#[tokio::test]
async fn the_delete_scan_refuses_a_climbing_row() {
    let engine = TestOperations::new();
    let inner = InMemoryVolume::new("Mem");
    inner.create_directory(app_path!("/album")).await.unwrap();
    inner.create_file(app_path!("/album/evil.txt"), b"EVIL").await.unwrap();
    inner.create_file(app_path!("/album/keep.txt"), b"KEEP").await.unwrap();
    inner.create_file(app_path!("/victim.txt"), b"VICTIM").await.unwrap();
    let volume = Arc::new(ClimbingRow { inner });
    let result = delete(&engine, Arc::clone(&volume) as Arc<dyn Volume>, "/album").await;
    assert!(result.is_err(), "a climbing row must stop the delete");
    assert!(
        volume.exists(app_path!("/victim.txt")).await,
        "nothing outside the selection goes"
    );
    assert!(
        volume.exists(app_path!("/album/keep.txt")).await,
        "nothing goes before the scan settles"
    );
}

/// A folder holding `foo\..` and `..\bar` deletes whole on every host: each is one name.
#[tokio::test]
async fn a_backslash_name_deletes_as_one_name() {
    let engine = TestOperations::new();
    let volume = Arc::new(InMemoryVolume::new("Mem"));
    volume.create_directory(app_path!("/album")).await.unwrap();
    volume.create_file(app_path!("/album/foo\\.."), b"ONE").await.unwrap();
    volume.create_file(app_path!("/album/..\\bar"), b"TWO").await.unwrap();
    volume.create_file(app_path!("/keep.txt"), b"KEEP").await.unwrap();
    delete(&engine, Arc::clone(&volume) as Arc<dyn Volume>, "/album")
        .await
        .expect("the delete runs");
    assert!(
        !volume.exists(app_path!("/album")).await,
        "the folder and both names went"
    );
    assert!(volume.exists(app_path!("/keep.txt")).await);
}

/// The same on a real disk (APFS, ext4): a file named `foo\..` deletes with its folder.
#[cfg(unix)]
#[tokio::test]
async fn a_backslash_name_on_a_real_disk_deletes_as_one_name() {
    let engine = TestOperations::new();
    let dir = tempfile::tempdir().expect("a scratch dir");
    let album = dir.path().join("album");
    std::fs::create_dir(&album).unwrap();
    std::fs::write(album.join("foo\\.."), b"ONE").unwrap();
    std::fs::write(album.join("..\\bar"), b"TWO").unwrap();
    let volume: Arc<dyn Volume> = Arc::new(cmdr_local::LocalVolume::local_folder(
        "Disk",
        "/",
        crate::host::testing::pane_order_host(),
    ));
    let source = album.to_str().expect("a UTF-8 tempdir").to_string();
    delete(&engine, volume, &source).await.expect("the delete runs");
    assert!(!album.exists(), "the folder and both names went");
    assert!(dir.path().exists());
}
