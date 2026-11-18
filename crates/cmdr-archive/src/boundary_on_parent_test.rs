//! The parent-aware boundary confirm, on `InMemoryVolume` parents: local ones
//! (`with_local_fs_access`, confirming on a real temp file) and remote ones
//! (confirming through the store's own `get_metadata` + `read_range`).

use std::path::{Path, PathBuf};

use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError};

use super::{confirm_archive_boundary_on_parent, confirm_remote_archive_boundary};

const ZIP_BYTES: &[u8] = b"PK\x03\x04not-a-real-archive-body";

/// A remote parent (no local FS access) holding `bytes` at `path`.
async fn remote_parent_with(path: &Path, bytes: &[u8]) -> InMemoryVolume {
    remote_volume_with(InMemoryVolume::new("Remote"), path, bytes).await
}

async fn remote_volume_with(parent: InMemoryVolume, path: &Path, bytes: &[u8]) -> InMemoryVolume {
    parent.create_file(path, bytes).await.expect("seed the remote file");
    parent
}

fn split(archive: &Path, inner: &str) -> Option<(PathBuf, PathBuf)> {
    Some((archive.to_path_buf(), PathBuf::from(inner)))
}

// ---- Local parent: `std::fs` ------------------------------------------------

#[tokio::test]
async fn a_local_parent_confirms_a_real_zip_and_its_inner_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let zip = dir.path().join("bundle.zip");
    std::fs::write(&zip, ZIP_BYTES).expect("write zip");
    let parent = InMemoryVolume::new("Local").with_local_fs_access();

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &zip.join("docs/readme.txt")).await,
        split(&zip, "docs/readme.txt")
    );
    // The archive file itself crosses with an empty inner path.
    assert_eq!(confirm_archive_boundary_on_parent(&parent, &zip).await, split(&zip, ""));
    assert_eq!(confirm_archive_boundary_on_parent(&parent, dir.path()).await, None);
}

#[tokio::test]
async fn a_local_parent_declines_a_folder_named_like_an_archive_and_a_mislabeled_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder = dir.path().join("folder.zip");
    std::fs::create_dir(&folder).expect("create folder named folder.zip");
    let mislabeled = dir.path().join("notreally.zip");
    std::fs::write(&mislabeled, b"plain text, definitely not a zip").expect("write file");
    let parent = InMemoryVolume::new("Local").with_local_fs_access();

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &folder.join("sub")).await,
        None
    );
    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &mislabeled.join("x")).await,
        None
    );
}

#[tokio::test]
async fn a_local_parent_confirms_on_disk_never_through_its_own_io() {
    // The fork is `supports_local_fs_access`: a local parent holding the zip only
    // in its own store (nothing on disk) does NOT confirm.
    let dir = tempfile::tempdir().expect("tempdir");
    let zip = dir.path().join("only-in-the-store.zip");
    let parent = remote_volume_with(InMemoryVolume::new("Local").with_local_fs_access(), &zip, ZIP_BYTES).await;

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &zip.join("inner")).await,
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
    let zip = PathBuf::from("/device/bundle.zip");
    let parent = remote_parent_with(&zip, ZIP_BYTES).await;

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &zip.join("docs/readme.txt")).await,
        split(&zip, "docs/readme.txt")
    );
    assert_eq!(confirm_archive_boundary_on_parent(&parent, &zip).await, split(&zip, ""));
    assert!(confirm_remote_archive_boundary(&parent, &zip).await);
    // One tar block, so a plain tar's ustar magic at offset 257 is in reach.
    assert_eq!(parent.read_range_log().first(), Some(&(0, 512)));
}

#[tokio::test]
async fn a_path_with_no_archive_component_costs_the_parent_no_read() {
    let parent = remote_parent_with(Path::new("/device/plain.txt"), b"text").await;

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, Path::new("/device/plain/dir")).await,
        None
    );
    assert!(parent.read_range_log().is_empty());
}

#[tokio::test]
async fn a_remote_folder_named_like_an_archive_declines() {
    let folder = PathBuf::from("/device/folder.zip");
    let parent = InMemoryVolume::new("Remote");
    parent.create_directory(&folder).await.expect("seed remote folder");

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &folder.join("sub")).await,
        None
    );
}

#[tokio::test]
async fn a_mislabeled_remote_file_declines() {
    let mislabeled = PathBuf::from("/device/notreally.zip");
    let parent = remote_parent_with(&mislabeled, b"plain text, definitely not a zip").await;

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &mislabeled.join("inner")).await,
        None
    );
}

#[tokio::test]
async fn a_missing_remote_archive_declines() {
    let parent = InMemoryVolume::new("Remote");

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, Path::new("/device/ghost.zip/inner")).await,
        None
    );
}

#[tokio::test]
async fn a_remote_confirm_checks_the_named_formats_magic() {
    let tgz = PathBuf::from("/device/real.tar.gz");
    let parent = remote_parent_with(&tgz, &[0x1f, 0x8b, 0x08, 0x00]).await;
    let zip_bytes_as_tgz = PathBuf::from("/device/fake.tar.gz");
    parent
        .create_file(&zip_bytes_as_tgz, ZIP_BYTES)
        .await
        .expect("seed fake tgz");

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &tgz.join("d/f.txt")).await,
        split(&tgz, "d/f.txt")
    );
    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &zip_bytes_as_tgz).await,
        None
    );
}

#[tokio::test]
async fn a_remote_parent_without_ranged_reads_still_routes() {
    // The primitive is missing, so the file can't be ruled out: route, and the
    // archive layer refuses typed if it's truly unreadable.
    let zip = PathBuf::from("/share/bundle.zip");
    let parent = remote_volume_with(
        InMemoryVolume::new("Remote").with_read_range_failing(VolumeError::NotSupported),
        &zip,
        ZIP_BYTES,
    )
    .await;

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &zip.join("inner")).await,
        split(&zip, "inner")
    );
}

#[tokio::test]
async fn a_transient_remote_sniff_fault_still_routes() {
    let zip = PathBuf::from("/share/bundle.zip");
    let fault = VolumeError::IoError {
        message: "connection reset".into(),
        raw_os_error: None,
    };
    let parent = remote_volume_with(
        InMemoryVolume::new("Remote").with_read_range_failing(fault),
        &zip,
        ZIP_BYTES,
    )
    .await;

    assert!(confirm_remote_archive_boundary(&parent, &zip).await);
}

#[tokio::test]
async fn a_remote_parent_confirms_through_its_own_io_never_on_disk() {
    // A real zip on disk at the path, but a remote parent that doesn't hold it,
    // doesn't confirm: its OS mount may be hung.
    let dir = tempfile::tempdir().expect("tempdir");
    let zip = dir.path().join("only-on-disk.zip");
    std::fs::write(&zip, ZIP_BYTES).expect("write zip");
    let parent = InMemoryVolume::new("Remote");

    assert_eq!(
        confirm_archive_boundary_on_parent(&parent, &zip.join("inner")).await,
        None
    );
}
