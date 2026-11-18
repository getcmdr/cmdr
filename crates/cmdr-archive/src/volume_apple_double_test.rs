//! `ArchiveVolume` hands an item's AppleDouble xattrs to whoever copies it: a
//! file's on its read stream (`VolumeReadStream::metadata`), a folder's through
//! `Volume::folder_metadata`, on a local parent and through a remote one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::TarCodec;
use crate::test_fixtures::{APPLE_DOUBLE_BSDTAR_TAR_GZ, APPLE_DOUBLE_DITTO_ZIP};
use cmdr_fs::metadata::MetadataClasses;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::InMemoryVolume;

const TAGS: &str = "com.apple.metadata:_kMDItemUserTags";

fn names(carried: &CarriedMetadata) -> Vec<String> {
    carried
        .xattrs
        .iter()
        .map(|xattr| xattr.name.to_string_lossy().into_owned())
        .collect()
}

/// The fixture on a local parent; the `TestDir` keeps it alive.
fn local_volume(bytes: &[u8], file_name: &str, format: ArchiveFormat) -> (TestDir, ArchiveVolume) {
    let dir = TestDir::new("archive-apple-double");
    let path = dir.join(file_name);
    std::fs::write(&path, bytes).expect("write the fixture");
    let parent = Arc::new(InMemoryVolume::new("parent").with_local_fs_access());
    let volume = ArchiveVolume::new(parent, path, format, VolumeHost::detached());
    (dir, volume)
}

async fn stream_xattrs(volume: &ArchiveVolume, inner: &str, wanted: MetadataClasses) -> Vec<String> {
    let stream = volume
        .open_read_stream(&AppPath::legacy_unchecked(Path::new(inner)))
        .await
        .expect("the entry opens");
    names(&stream.metadata(wanted))
}

#[tokio::test]
async fn a_finder_made_zip_carries_tags_on_files_and_folders() {
    let (_dir, volume) = local_volume(APPLE_DOUBLE_DITTO_ZIP, "tagged.zip", ArchiveFormat::Zip);

    let root = volume
        .list_directory(&AppPath::legacy_unchecked(Path::new("")), None)
        .await
        .expect("lists");
    let listed: Vec<&str> = root.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(listed, vec!["tagged-dir", "plain.txt", "tagged.txt"]);

    assert_eq!(
        stream_xattrs(&volume, "tagged.txt", MetadataClasses::ALL).await,
        vec![TAGS]
    );
    assert!(
        stream_xattrs(&volume, "plain.txt", MetadataClasses::ALL)
            .await
            .is_empty()
    );
    assert!(
        stream_xattrs(&volume, "tagged.txt", MetadataClasses::MODE)
            .await
            .is_empty(),
        "a destination that keeps no xattrs is handed none"
    );

    let folder = volume
        .folder_metadata(
            &AppPath::legacy_unchecked(Path::new("tagged-dir")),
            MetadataClasses::ALL,
        )
        .await;
    assert_eq!(names(&folder), vec![TAGS]);
    assert!(
        volume
            .folder_metadata(
                &AppPath::legacy_unchecked(Path::new("tagged-dir")),
                MetadataClasses::MODE
            )
            .await
            .is_empty()
    );

    // The sidecars themselves never open as entries of their own.
    assert!(
        volume
            .open_read_stream(&AppPath::legacy_unchecked(Path::new("__MACOSX/._tagged.txt")))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_remote_zip_reads_its_sidecars_through_the_parent() {
    let parent = Arc::new(InMemoryVolume::new("remote"));
    let archive_path = PathBuf::from("/remote/tagged.zip");
    parent
        .create_file(&AppPath::legacy_unchecked(&archive_path), APPLE_DOUBLE_DITTO_ZIP)
        .await
        .expect("load the zip into the parent");
    let volume = ArchiveVolume::new(
        Arc::clone(&parent) as Arc<dyn Volume>,
        archive_path,
        ArchiveFormat::Zip,
        VolumeHost::detached(),
    );
    assert_eq!(
        stream_xattrs(&volume, "tagged.txt", MetadataClasses::ALL).await,
        vec![TAGS]
    );
    assert_eq!(
        names(
            &volume
                .folder_metadata(
                    &AppPath::legacy_unchecked(Path::new("tagged-dir")),
                    MetadataClasses::ALL
                )
                .await
        ),
        vec![TAGS]
    );
}

#[tokio::test]
async fn a_macos_tar_carries_tags_through_the_one_pass_extract_and_on_folders() {
    let (_dir, volume) = local_volume(
        APPLE_DOUBLE_BSDTAR_TAR_GZ,
        "tagged.tar.gz",
        ArchiveFormat::Tar(TarCodec::Gzip),
    );
    let mut extract = volume
        .open_sequential_extract(&AppPath::legacy_unchecked(Path::new("")))
        .await
        .expect("the extract opens");
    let mut tagged = Vec::new();
    while let Some(file) = extract.next_file().await.expect("next member") {
        let carried = extract.current_stream().metadata(MetadataClasses::ALL);
        if !carried.xattrs.is_empty() {
            tagged.push(
                file.source_path
                    .as_std_legacy()
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned()),
            );
        }
    }
    assert_eq!(tagged, vec![Some("tagged.txt".to_string())]);
    assert_eq!(
        names(
            &volume
                .folder_metadata(
                    &AppPath::legacy_unchecked(Path::new("tagged-dir")),
                    MetadataClasses::ALL
                )
                .await
        ),
        vec![TAGS]
    );
}
