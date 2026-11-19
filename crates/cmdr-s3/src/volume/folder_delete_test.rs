//! A folder with no marker beside a file of its name (`notes` and `notes/…`,
//! which `aws s3 sync` leaves), deleted the way the engine deletes a folder:
//! its children, then [`Volume::delete_directory`]. Once the last child goes,
//! nothing holds the folder's name but the file, and ❗ the folder's delete
//! must never reach it, whether or not a listing ever showed it as
//! `notes (file)`.
//!
//! Over `fake_s3.rs`: the bug needs a real `notes (file)` key shadowing the
//! file's row, which the fake seeds in one line.

use std::time::Duration;

use cmdr_fs::volume::{Volume, VolumeError};

use super::fake_s3::FakeS3;
use super::test_support::at;

const FILE: &str = "p/notes";
const CHILD: &str = "p/notes/a.txt";
const SHADOW: &str = "p/notes (file)";

/// `p/notes` (a file), `p/notes/a.txt` (the markerless folder's one child),
/// and a real `p/notes (file)`, which takes the name the file would list as,
/// so the listing hides the file.
async fn seeded() -> FakeS3 {
    let s3 = FakeS3::start(Duration::ZERO).await;
    s3.seed(FILE, 8);
    s3.seed(CHILD, 5);
    s3.seed(SHADOW, 3);
    s3
}

/// The engine's order for a folder: its child, then the folder itself.
async fn delete_folder_like_the_engine(volume: &dyn Volume) {
    volume.delete(&at(volume, CHILD)).await.expect("the child deletes");
    let folder = volume.delete_directory(&at(volume, FILE), None).await;
    assert!(
        matches!(folder, Ok(()) | Err(VolumeError::NotFound(_))),
        "a folder that left with its last child is fine either way: {folder:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_listed_markerless_folder_deletes_without_the_hidden_file_of_its_name() {
    let s3 = seeded().await;
    let volume = s3.volume();
    let listed = volume.list_directory(&at(&volume, "p"), None).await.expect("p lists");
    assert!(
        listed.iter().any(|entry| entry.name == "notes" && entry.is_directory),
        "the folder row: {listed:?}"
    );

    delete_folder_like_the_engine(&volume).await;

    assert!(s3.object(FILE).is_some(), "the file nobody selected survives");
    assert!(s3.object(SHADOW).is_some(), "and so does `notes (file)`");
    assert!(s3.object(CHILD).is_none(), "the folder's child went");
}

/// A volume rebuilt under a live pane (re-adding a connected place) has never
/// listed `p`, so it knows nothing about what sits beside the folder.
#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_volume_deletes_a_markerless_folder_without_the_file_of_its_name() {
    let s3 = seeded().await;
    let volume = s3.volume();

    delete_folder_like_the_engine(&volume).await;

    assert!(s3.object(FILE).is_some(), "the file nobody selected survives");
    assert!(s3.object(CHILD).is_none(), "the folder's child went");
}

/// The listing hides the file but still knows it's there, so the folder row
/// means the folder alone, as it does when the file lists as `notes (file)`.
#[tokio::test(flavor = "multi_thread")]
async fn the_folder_row_beside_a_hidden_file_is_only_the_folder() {
    let s3 = seeded().await;
    let volume = s3.volume();
    volume.list_directory(&at(&volume, "p"), None).await.expect("p lists");

    let folder = volume
        .get_metadata(&at(&volume, FILE))
        .await
        .expect("the folder row stats");
    assert!(folder.is_directory, "the folder row stats as the folder: {folder:?}");
    let read = volume.open_read_stream(&at(&volume, FILE)).await.map(|_| ());
    assert!(
        matches!(read, Err(VolumeError::IsADirectory(_))),
        "the folder row never reads as the file: {read:?}"
    );
    let shadow = volume
        .get_metadata(&at(&volume, SHADOW))
        .await
        .expect("the real key stats");
    assert_eq!(shadow.size, Some(3), "`notes (file)` is still its own key");

    // A plain delete of the emptied folder row, as a backend-agnostic caller
    // would send it, still never reaches the file.
    volume.delete(&at(&volume, CHILD)).await.expect("the child deletes");
    let deleted = volume.delete(&at(&volume, FILE)).await;
    assert!(matches!(deleted, Err(VolumeError::NotFound(_))), "{deleted:?}");
    assert!(s3.object(FILE).is_some(), "the file survives");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_delete_takes_an_empty_folders_marker() {
    let s3 = FakeS3::start(Duration::ZERO).await;
    s3.seed("p/empty/", 0);
    let volume = s3.volume();

    let deleted = volume.delete_directory(&at(&volume, "p/empty"), None).await;

    assert!(deleted.is_ok(), "{deleted:?}");
    assert!(s3.object("p/empty/").is_none(), "the marker went");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_delete_never_takes_a_file() {
    let s3 = FakeS3::start(Duration::ZERO).await;
    s3.seed("p/plain.txt", 4);
    let volume = s3.volume();

    let deleted = volume.delete_directory(&at(&volume, "p/plain.txt"), None).await;

    assert!(matches!(deleted, Err(VolumeError::NotFound(_))), "{deleted:?}");
    assert!(s3.object("p/plain.txt").is_some(), "the file stays");
}
