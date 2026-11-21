//! The parent-aware boundary confirm, on `InMemoryVolume` parents: local ones
//! (`with_local_fs_access`, confirming on a real temp file) and remote ones
//! (confirming through the store's own `get_metadata` + `read_range`).

use cmdr_fs::app_path;
use cmdr_fs::app_path::{AppPath, AppPathBuf, RelativeAppPath, RelativeAppPathBuf};
use cmdr_fs::testing::AppTestDir;

use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError};

use super::{confirm_archive_boundary_on_parent_at, confirm_remote_archive_boundary_at};
use crate::testing::at;

const ZIP_BYTES: &[u8] = b"PK\x03\x04not-a-real-archive-body";

/// A remote parent (no local FS access) holding `bytes` at `path`.
async fn remote_parent_with(path: &AppPath, bytes: &[u8]) -> InMemoryVolume {
    remote_volume_with(InMemoryVolume::new("Remote"), path, bytes).await
}

async fn remote_volume_with(parent: InMemoryVolume, path: &AppPath, bytes: &[u8]) -> InMemoryVolume {
    parent.create_file(path, bytes).await.expect("seed the remote file");
    parent
}

fn split(archive: &AppPath, inner: &str) -> Option<(AppPathBuf, RelativeAppPathBuf)> {
    let inner = RelativeAppPath::new(inner).expect("a relative inner path").to_owned();
    Some((archive.to_owned(), inner))
}

/// A scratch dir holding `name` with `bytes`, and that file's app path.
fn on_disk(name: &str, bytes: &[u8]) -> (AppTestDir, AppPathBuf) {
    let dir = AppTestDir::new("archive-boundary");
    std::fs::write(dir.native().join(name), bytes).expect("write the file");
    let path = at(dir.app_path(), name);
    (dir, path)
}

// ---- Local parent: `std::fs` ------------------------------------------------

#[tokio::test]
async fn a_local_parent_confirms_a_real_zip_and_its_inner_paths() {
    let (dir, zip) = on_disk("bundle.zip", ZIP_BYTES);
    let parent = InMemoryVolume::new("Local").with_local_fs_access();

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(&zip, "docs/readme.txt")).await,
        split(&zip, "docs/readme.txt")
    );
    // The archive file itself crosses with an empty inner path.
    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &zip).await,
        split(&zip, "")
    );
    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, dir.app_path()).await,
        None
    );
}

#[tokio::test]
async fn a_local_parent_declines_a_folder_named_like_an_archive_and_a_mislabeled_file() {
    let dir = AppTestDir::new("archive-boundary");
    std::fs::create_dir(dir.native().join("folder.zip")).expect("create folder named folder.zip");
    std::fs::write(dir.native().join("notreally.zip"), b"plain text, definitely not a zip").expect("write file");
    let parent = InMemoryVolume::new("Local").with_local_fs_access();

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(dir.app_path(), "folder.zip/sub")).await,
        None
    );
    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(dir.app_path(), "notreally.zip/x")).await,
        None
    );
}

#[tokio::test]
async fn a_local_parent_confirms_on_disk_never_through_its_own_io() {
    // The fork is `supports_local_fs_access`: a local parent holding the zip only
    // in its own store (nothing on disk) does NOT confirm.
    let dir = AppTestDir::new("archive-boundary");
    let zip = at(dir.app_path(), "only-in-the-store.zip");
    let parent = remote_volume_with(InMemoryVolume::new("Local").with_local_fs_access(), &zip, ZIP_BYTES).await;

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(&zip, "inner")).await,
        None
    );
    assert!(
        parent.read_range_log().is_empty(),
        "a local confirm never reads through the parent"
    );
}

// ---- Remote parent: the parent's own I/O ------------------------------------

#[tokio::test]
async fn a_remote_parent_confirms_a_zip_through_its_own_reads() {
    let zip = app_path!("/device/bundle.zip");
    let parent = remote_parent_with(zip, ZIP_BYTES).await;

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(zip, "docs/readme.txt")).await,
        split(zip, "docs/readme.txt")
    );
    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, zip).await,
        split(zip, "")
    );
    assert!(confirm_remote_archive_boundary_at(&parent, zip).await);
    // One tar block, so a plain tar's ustar magic at offset 257 is in reach.
    assert_eq!(parent.read_range_log().first(), Some(&(0, 512)));
}

#[tokio::test]
async fn a_path_with_no_archive_component_costs_the_parent_no_read() {
    let parent = remote_parent_with(app_path!("/device/plain.txt"), b"text").await;

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, app_path!("/device/plain/dir")).await,
        None
    );
    assert!(parent.read_range_log().is_empty());
}

#[tokio::test]
async fn a_remote_folder_named_like_an_archive_declines() {
    let folder = app_path!("/device/folder.zip");
    let parent = InMemoryVolume::new("Remote");
    parent.create_directory(folder).await.expect("seed remote folder");

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(folder, "sub")).await,
        None
    );
}

#[tokio::test]
async fn a_mislabeled_remote_file_declines() {
    let mislabeled = app_path!("/device/notreally.zip");
    let parent = remote_parent_with(mislabeled, b"plain text, definitely not a zip").await;

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(mislabeled, "inner")).await,
        None
    );
}

#[tokio::test]
async fn a_missing_remote_archive_declines() {
    let parent = InMemoryVolume::new("Remote");

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, app_path!("/device/ghost.zip/inner")).await,
        None
    );
}

#[tokio::test]
async fn a_remote_confirm_checks_the_named_formats_magic() {
    let tgz = app_path!("/device/real.tar.gz");
    let parent = remote_parent_with(tgz, &[0x1f, 0x8b, 0x08, 0x00]).await;
    let zip_bytes_as_tgz = app_path!("/device/fake.tar.gz");
    parent
        .create_file(zip_bytes_as_tgz, ZIP_BYTES)
        .await
        .expect("seed fake tgz");

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(tgz, "d/f.txt")).await,
        split(tgz, "d/f.txt")
    );
    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, zip_bytes_as_tgz).await,
        None
    );
}

#[tokio::test]
async fn a_remote_parent_without_ranged_reads_still_routes() {
    // The primitive is missing, so the file can't be ruled out: route, and the
    // archive layer refuses typed if it's truly unreadable.
    let zip = app_path!("/share/bundle.zip");
    let parent = remote_volume_with(
        InMemoryVolume::new("Remote").with_read_range_failing(VolumeError::NotSupported),
        zip,
        ZIP_BYTES,
    )
    .await;

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(zip, "inner")).await,
        split(zip, "inner")
    );
}

#[tokio::test]
async fn a_transient_remote_sniff_fault_still_routes() {
    let zip = app_path!("/share/bundle.zip");
    let fault = VolumeError::IoError {
        message: "connection reset".into(),
        raw_os_error: None,
    };
    let parent = remote_volume_with(
        InMemoryVolume::new("Remote").with_read_range_failing(fault),
        zip,
        ZIP_BYTES,
    )
    .await;

    assert!(confirm_remote_archive_boundary_at(&parent, zip).await);
}

#[tokio::test]
async fn a_remote_parent_confirms_through_its_own_io_never_on_disk() {
    // A real zip on disk at the path, but a remote parent that doesn't hold it,
    // doesn't confirm: its OS mount may be hung.
    let (_dir, zip) = on_disk("only-on-disk.zip", ZIP_BYTES);
    let parent = InMemoryVolume::new("Remote");

    assert_eq!(
        confirm_archive_boundary_on_parent_at(&parent, &at(&zip, "inner")).await,
        None
    );
}

/// The `std::path` forms the host still calls answer what the typed ones do, on
/// the same parents: a local confirm, a remote one, and the remote sniff alone.
/// Unix only: they take the POSIX text those callers hand in.
#[cfg(unix)]
#[tokio::test]
async fn the_std_path_forms_answer_as_the_typed_ones() {
    use super::{confirm_archive_boundary_on_parent, confirm_remote_archive_boundary};
    use std::path::{Path, PathBuf};

    let (dir, _zip) = on_disk("bundle.zip", ZIP_BYTES);
    let local = InMemoryVolume::new("Local").with_local_fs_access();
    let on_disk_zip = dir.native().join("bundle.zip");
    assert_eq!(
        confirm_archive_boundary_on_parent(&local, &on_disk_zip.join("docs/readme.txt")).await,
        Some((on_disk_zip.clone(), PathBuf::from("docs/readme.txt")))
    );

    let remote = remote_parent_with(app_path!("/device/bundle.zip"), ZIP_BYTES).await;
    assert_eq!(
        confirm_archive_boundary_on_parent(&remote, Path::new("/device/bundle.zip/a/b")).await,
        Some((PathBuf::from("/device/bundle.zip"), PathBuf::from("a/b")))
    );
    assert!(confirm_remote_archive_boundary(&remote, Path::new("/device/bundle.zip")).await);
    assert!(!confirm_remote_archive_boundary(&remote, Path::new("/device/ghost.zip")).await);
}
