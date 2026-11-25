//! Extracting a Finder-made zip or a macOS tar keeps each item's Finder tags
//! (and every other xattr its AppleDouble sidecar carries) on files AND folders,
//! and lands no `__MACOSX` folder and no `._` files: what `ditto -x` and bsdtar
//! do (`docs/notes/transfer-metadata-experiments-2026-10-11.md` § "The quirks").
//! The fixtures are the checked-in `cmdr_archive::test_fixtures` archives.

use super::tests::make_state;
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use cmdr_archive::ArchiveFormat;
use cmdr_fs::testing::metadata_fixture::{QUARANTINE_VALUE, QUARANTINE_XATTR, TAGS_XATTR, set_xattr};
use cmdr_fs::volume::InMemoryVolume;
use cmdr_local::LocalPosixVolume;
use std::path::Path;

/// An `ArchiveVolume` over `archive` on a local-backed parent, on a lane of its
/// own.
fn local_archive_volume(archive: &Path, format: ArchiveFormat) -> Arc<dyn Volume> {
    use cmdr_archive::ArchiveVolume;
    use cmdr_fs::volume::host::VolumeHost;
    let parent: Arc<dyn Volume> = Arc::new(
        InMemoryVolume::new("Parent")
            .with_local_fs_access()
            .with_lane_key(format!("m8-apple-double-{}", uuid::Uuid::new_v4())),
    );
    Arc::new(ArchiveVolume::new(
        parent,
        archive.to_path_buf(),
        format,
        VolumeHost::detached(),
    ))
}

async fn copy_out(source: &Arc<dyn Volume>, sources: &[PathBuf], dest_dir: &Path, state: &Arc<WriteOperationState>) {
    let dest: Arc<dyn Volume> = Arc::new(LocalPosixVolume::local_folder(
        "Dest",
        dest_dir.to_str().expect("dest path"),
        crate::host::testing::pane_order_host(),
    ));
    let result = copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "extract-apple-double-op",
        state,
        Arc::clone(source),
        &crate::testing::app_paths_of(sources),
        dest,
        &crate::testing::app_path_of(Path::new("/")),
        &VolumeCopyConfig {
            progress_interval_ms: 0,
            ..VolumeCopyConfig::default()
        },
    )
    .await;
    assert!(result.is_ok(), "the extract should succeed: {result:?}");
}

fn xattr_of(path: &Path, name: &str) -> Option<Vec<u8>> {
    xattr::get(path, name).unwrap_or_else(|e| panic!("read {name} of {}: {e}", path.display()))
}

/// Every name under `dir`, recursively, relative to it.
fn landed_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = walkdir::WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .map(|entry| {
            let entry = entry.expect("walk the destination");
            entry
                .path()
                .strip_prefix(dir)
                .expect("under the destination")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// The same tree comes out of the zip and the tar: tags on the tagged file and
/// folder, none on the rest, and nothing but the items themselves.
fn assert_tagged_tree_landed(dest: &Path) {
    assert_eq!(
        landed_names(dest),
        vec!["plain.txt", "tagged-dir", "tagged-dir/inner.txt", "tagged.txt"],
        "no `__MACOSX` and no `._` sidecar lands as a file"
    );
    for tagged in ["tagged.txt", "tagged-dir"] {
        let tags = xattr_of(&dest.join(tagged), TAGS_XATTR).unwrap_or_else(|| panic!("tags missing on {tagged}"));
        assert!(tags.starts_with(b"bplist00"), "{tagged}: the tags are the stored plist");
    }
    for plain in ["plain.txt", "tagged-dir/inner.txt"] {
        assert_eq!(xattr_of(&dest.join(plain), TAGS_XATTR), None, "{plain} had no tags");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extracting_a_finder_made_zip_keeps_tags_on_files_and_folders() {
    let engine = TestOperations::new();
    let tmp = tempfile::tempdir().expect("tempdir");
    let zip_path = tmp.path().join("tagged.zip");
    std::fs::write(&zip_path, cmdr_archive::test_fixtures::APPLE_DOUBLE_DITTO_ZIP).expect("write the zip");
    let dest = tempfile::tempdir().expect("dest tempdir");

    let source = local_archive_volume(&zip_path, ArchiveFormat::Zip);
    let sources = ["tagged.txt", "plain.txt", "tagged-dir"].map(|name| zip_path.join(name));
    copy_out(&source, &sources, dest.path(), &make_state(&engine)).await;

    assert_tagged_tree_landed(dest.path());
}

/// A compressed tar takes the one-pass sequential extract for a folder, and the
/// prefix-decoding single-entry read for a file selected alone: both carry tags.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extracting_a_macos_tar_keeps_tags_on_files_and_folders() {
    let engine = TestOperations::new();
    let tmp = tempfile::tempdir().expect("tempdir");
    let tar_path = tmp.path().join("tagged.tar.gz");
    std::fs::write(&tar_path, cmdr_archive::test_fixtures::APPLE_DOUBLE_BSDTAR_TAR_GZ).expect("write the tar");
    let dest = tempfile::tempdir().expect("dest tempdir");

    let source = local_archive_volume(&tar_path, ArchiveFormat::Tar(cmdr_archive::TarCodec::Gzip));
    let sources = ["tagged.txt", "plain.txt", "tagged-dir"].map(|name| tar_path.join(name));
    copy_out(&source, &sources, dest.path(), &make_state(&engine)).await;

    assert_tagged_tree_landed(dest.path());
}

/// A sidecar is the archive author's word, and a quarantine in it could claim
/// the user already approved the app. The archive's own quarantine wins on every
/// file and folder that lands, whatever the sidecar says.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sidecar_quarantine_never_overrides_the_archives_own() {
    use cmdr_archive::test_fixtures::{apple_double_sidecar, build_zip, stored};

    let engine = TestOperations::new();
    const FORGED: &[u8] = b"0043;5f000000;Safari;00000000-0000-0000-0000-000000000000";
    let forged = apple_double_sidecar(&[(QUARANTINE_XATTR, FORGED), (TAGS_XATTR, b"bplist00 tags")]);

    let tmp = tempfile::tempdir().expect("tempdir");
    let zip_path = tmp.path().join("download.zip");
    std::fs::write(
        &zip_path,
        build_zip(&[
            stored("app/run.sh", "#!/bin/sh\n"),
            stored("__MACOSX/._app", forged.clone()),
            stored("__MACOSX/app/._run.sh", forged),
        ]),
    )
    .expect("write the zip");
    set_xattr(&zip_path, QUARANTINE_XATTR, QUARANTINE_VALUE);
    let dest = tempfile::tempdir().expect("dest tempdir");

    let source = local_archive_volume(&zip_path, ArchiveFormat::Zip);
    copy_out(&source, &[zip_path.join("app")], dest.path(), &make_state(&engine)).await;

    for landed in ["app", "app/run.sh"] {
        let path = dest.path().join(landed);
        assert_eq!(
            xattr_of(&path, QUARANTINE_XATTR).as_deref(),
            Some(QUARANTINE_VALUE),
            "the archive's quarantine on {landed}"
        );
        assert_eq!(
            xattr_of(&path, TAGS_XATTR).as_deref(),
            Some(b"bplist00 tags".as_slice()),
            "the rest of the sidecar on {landed}"
        );
    }
}
