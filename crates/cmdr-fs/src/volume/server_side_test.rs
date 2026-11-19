//! The default subtree tally and the trait defaults around a rename that copies.

use crate::app_path;
use crate::app_path::AppPath;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use super::*;
use crate::volume::{InMemoryVolume, WriteMode};

async fn volume_with_tree() -> InMemoryVolume {
    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/tree")).await.unwrap();
    volume.create_directory(app_path!("/tree/sub")).await.unwrap();
    volume.create_file(app_path!("/tree/a.txt"), b"aaaa").await.unwrap();
    volume.create_file(app_path!("/tree/sub/b.txt"), b"bb").await.unwrap();
    volume.create_file(app_path!("/tree/sub/c.txt"), b"c").await.unwrap();
    volume
}

#[tokio::test]
async fn a_file_tallies_as_one_file_of_its_size() {
    let volume = volume_with_tree().await;
    let tally = volume.tally_subtree(app_path!("/tree/a.txt"), 100).await.unwrap();
    assert_eq!(
        (tally.files, tally.bytes, tally.folders, tally.complete),
        (1, 4, 0, true)
    );
    assert_eq!(tally.per_file.iter().map(|file| file.size).collect::<Vec<_>>(), [4]);
}

#[tokio::test]
async fn a_folder_tallies_every_file_at_any_depth() {
    let volume = volume_with_tree().await;
    let tally = volume.tally_subtree(app_path!("/tree"), 100).await.unwrap();
    assert_eq!((tally.files, tally.bytes, tally.complete), (3, 7, true));
    assert_eq!(tally.folders, 2, "the folder itself and `sub`");
    let mut sizes: Vec<u64> = tally.per_file.iter().map(|file| file.size).collect();
    sizes.sort_unstable();
    assert_eq!(
        sizes,
        [1, 2, 4],
        "each counted file's size, for the rename's cost estimate"
    );
}

/// The cap is what keeps F2 on a huge folder from listing all of it before the
/// editor answers.
#[tokio::test]
async fn the_tally_stops_past_its_cap_and_says_so() {
    let volume = volume_with_tree().await;
    let tally = volume.tally_subtree(app_path!("/tree"), 2).await.unwrap();
    assert_eq!(tally.files, 2);
    assert!(!tally.complete, "a third file exists past the cap");
}

#[tokio::test]
async fn a_tally_of_nothing_is_not_found() {
    let volume = volume_with_tree().await;
    assert!(matches!(
        volume.tally_subtree(app_path!("/nope"), 10).await,
        Err(VolumeError::NotFound(_))
    ));
}

#[tokio::test]
async fn a_volume_renames_in_one_call_unless_it_says_otherwise() {
    let plain = volume_with_tree().await;
    assert_eq!(
        plain.rename_work(app_path!("/tree")).await.unwrap(),
        RenameWork::OneCall
    );
    let store = InMemoryVolume::new("Store").with_renames_by_copy();
    assert_eq!(
        store.rename_work(app_path!("/tree")).await.unwrap(),
        RenameWork::CopyThenDelete
    );
}

/// The batch default deletes each path and calls a path already gone done.
#[tokio::test]
async fn the_default_batch_delete_answers_per_path_in_order() {
    let volume = volume_with_tree().await;
    let paths = [
        PathBuf::from("/tree/a.txt"),
        PathBuf::from("/tree/gone.txt"),
        PathBuf::from("/tree/sub/b.txt"),
    ];
    let results = volume
        .delete_files(
            &paths
                .iter()
                .map(|p| AppPath::legacy_unchecked(p).into_owned())
                .collect::<Vec<_>>(),
        )
        .await;
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert!(!volume.exists(app_path!("/tree/a.txt")).await);
    assert!(!volume.exists(app_path!("/tree/sub/b.txt")).await);
    assert!(volume.exists(app_path!("/tree/sub/c.txt")).await);
}

struct Silent;

impl ServerCopyProgress for Silent {
    fn advanced(&self, _done: u64, _total: u64) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn checkpoint(&self) -> Pin<Box<dyn Future<Output = ControlFlow<()>> + Send + '_>> {
        Box::pin(async { ControlFlow::Continue(()) })
    }
}

/// ❗ The default never copies across two volumes: a path on one server copied
/// on another is the wrong file.
#[tokio::test]
async fn the_default_server_copy_refuses_another_volume_and_a_no_overwrite_write() {
    let one = volume_with_tree().await;
    let other = volume_with_tree().await;
    let from = Path::new("/tree/a.txt");
    let to = Path::new("/tree/copy.txt");
    assert!(matches!(
        one.copy_on_server(
            &other,
            &AppPath::legacy_unchecked(from),
            &AppPath::legacy_unchecked(to),
            WriteMode::CreateOrReplace,
            &Silent
        )
        .await,
        Err(VolumeError::NotSupported)
    ));
    assert!(matches!(
        one.copy_on_server(
            &one,
            &AppPath::legacy_unchecked(from),
            &AppPath::legacy_unchecked(to),
            WriteMode::CreateNew,
            &Silent
        )
        .await,
        Err(VolumeError::NotSupported)
    ));
}
