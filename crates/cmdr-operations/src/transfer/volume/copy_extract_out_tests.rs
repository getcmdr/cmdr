//! Extract-out tests for `copy_volumes_with_progress`, split out of
//! `volume/copy_tests/`. A cross-volume copy that pulls a file and a
//! directory subtree OUT of a zip archive through the transfer engine, plus the
//! Zip-Slip guard that writes a symlink entry as a regular file.
//!
//! Shared fixture `make_state` lives in `volume/copy_tests/mod.rs` (`super::tests`).
#![cfg(unix)] // POSIX-only cells: `LocalPosixVolume`, POSIX modes, or errnos (windows-port plan, tasks 35 and 36).

use super::tests::make_state;
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use cmdr_fs::volume::InMemoryVolume;
use cmdr_local::LocalPosixVolume;

// ========================================================================
// Extract-out: copy a file + a directory subtree OUT of a zip archive
// (headless repro of the extract-out flow through the transfer engine).
// ========================================================================

/// Builds a real zip with a top-level file and a two-file directory, returning
/// the tempdir (keep it alive) and the `.zip` path.
fn build_extract_out_fixture() -> (tempfile::TempDir, PathBuf) {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let zip_path = dir.path().join("bundle.zip");
    let file = std::fs::File::create(&zip_path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("readme.txt", options).expect("start readme");
    writer.write_all(b"hello").expect("write readme");
    writer.add_directory("docs/", options).expect("add docs dir");
    writer.start_file("docs/a.txt", options).expect("start a");
    writer.write_all(b"aaa").expect("write a");
    writer.start_file("docs/b.txt", options).expect("start b");
    writer.write_all(b"bbb").expect("write b");
    writer.finish().expect("finish zip");
    (dir, zip_path)
}

async fn read_dest_file(dest: &Arc<dyn Volume>, path: &str) -> Vec<u8> {
    let mut stream = dest
        .open_read_stream(Path::new(path))
        .await
        .unwrap_or_else(|e| panic!("dest missing {path}: {e:?}"));
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        out.extend_from_slice(&chunk.expect("chunk"));
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extract_out_copies_a_file_and_a_directory_subtree_out_of_a_zip() {
    use cmdr_archive::{ArchiveFormat, ArchiveVolume};
    use cmdr_fs::volume::host::VolumeHost;

    let engine = TestOperations::new();

    let (_tmp, zip_path) = build_extract_out_fixture();

    // Source = the read-only ArchiveVolume over the zip; dest = in-memory.
    let parent: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Parent").with_local_fs_access());
    let source: Arc<dyn Volume> = Arc::new(ArchiveVolume::new(
        parent,
        zip_path.clone(),
        ArchiveFormat::Zip,
        VolumeHost::detached(),
    ));
    let dest: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));

    let events = Arc::new(CollectorEventSink::new());
    let state = make_state(&engine);
    let config = VolumeCopyConfig {
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };

    // The FE sends FULL paths that cross the archive boundary (what resolve
    // returns unchanged): a top-level file and a directory.
    let sources = vec![zip_path.join("readme.txt"), zip_path.join("docs")];

    let result = copy_volumes_with_progress(
        events.clone(),
        "extract-out-op",
        &state,
        Arc::clone(&source),
        &sources,
        Arc::clone(&dest),
        Path::new("/"),
        &config,
    )
    .await;

    assert!(result.is_ok(), "extract-out should succeed: {result:?}");

    // The file and both subtree files land at the destination with their bytes.
    assert_eq!(read_dest_file(&dest, "/readme.txt").await, b"hello");
    assert_eq!(read_dest_file(&dest, "/docs/a.txt").await, b"aaa");
    assert_eq!(read_dest_file(&dest, "/docs/b.txt").await, b"bbb");

    let complete = events.complete.lock().unwrap();
    assert_eq!(complete.len(), 1, "one completion event");
    // `files_processed` counts FILES (the top-level one + the directory's two),
    // the same as a local copy and as the bar's own denominator.
    assert_eq!(complete[0].files_processed, 3, "every file the copy landed");
    assert_eq!(complete[0].bytes_processed, 5 + 3 + 3, "all inner-file bytes copied");
}

/// A script extracted out of a release zip has to still RUN. The archive records
/// the mode and the volume reports it; the copy engine is the layer that has to
/// put it on what lands.
///
/// Both shapes, because they take different routes through the engine: the entry
/// selected ALONE goes through the top-level dispatch (which has no listing in
/// hand and asks the source), the one inside a folder through the merge walker
/// (which has the entry already). The dest is a REAL local volume, because a
/// mode is only a fact on a filesystem that has them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_executable_bit_survives_an_extract_out_of_a_zip() {
    use cmdr_archive::test_fixtures::{build_zip, stored, with_unix_mode};
    use cmdr_archive::{ArchiveFormat, ArchiveVolume};
    use cmdr_fs::volume::host::VolumeHost;

    let engine = TestOperations::new();

    let tmp = tempfile::tempdir().expect("tempdir");
    let zip_path = tmp.path().join("release.zip");
    std::fs::write(
        &zip_path,
        build_zip(&[
            with_unix_mode("run.sh", "#!/bin/sh\n", 0o755),
            with_unix_mode("scripts/run.sh", "#!/bin/sh\n", 0o755),
            stored("scripts/notes.txt", "nnn"),
        ]),
    )
    .expect("write the fixture zip");

    let dest_dir = tempfile::tempdir().expect("dest tempdir");
    let parent: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Parent").with_local_fs_access());
    let source: Arc<dyn Volume> = Arc::new(ArchiveVolume::new(
        parent,
        zip_path.clone(),
        ArchiveFormat::Zip,
        VolumeHost::detached(),
    ));
    let dest: Arc<dyn Volume> = Arc::new(LocalPosixVolume::local_folder(
        "Dest",
        dest_dir.path().to_str().expect("dest path"),
        crate::host::testing::pane_order_host(),
    ));

    let sources = vec![zip_path.join("run.sh"), zip_path.join("scripts")];
    let result = copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "extract-out-exec-bit-op",
        &make_state(&engine),
        Arc::clone(&source),
        &sources,
        Arc::clone(&dest),
        Path::new("/"),
        &VolumeCopyConfig {
            progress_interval_ms: 0,
            ..VolumeCopyConfig::default()
        },
    )
    .await;
    assert!(result.is_ok(), "extract-out should succeed: {result:?}");

    assert_eq!(
        mode_of(&dest_dir.path().join("run.sh")) & 0o111,
        0o111,
        "an entry selected on its own has to land executable"
    );
    assert_eq!(
        mode_of(&dest_dir.path().join("scripts/run.sh")) & 0o111,
        0o111,
        "and so does one the merge walker found inside a folder"
    );
    assert_eq!(
        mode_of(&dest_dir.path().join("scripts/notes.txt")) & 0o111,
        0,
        "a plain entry stays non-executable: the mode is carried, ❌ never widened"
    );
}

/// The permission bits `path` wears right now.
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("stat {}: {e}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extracting_a_symlink_entry_writes_a_regular_file_never_a_symlink() {
    use cmdr_archive::{ArchiveFormat, ArchiveVolume};
    use cmdr_fs::volume::host::VolumeHost;

    let engine = TestOperations::new();

    // Pins that extraction never CREATES a symlink from archive data: a symlink
    // entry's content is its target path, and writing those bytes verbatim as a
    // regular file is what stops Zip Slip through the back door (a symlink entry
    // pointing outside the extraction root).
    let src_dir = tempfile::tempdir().expect("src tempdir");
    let zip_path = src_dir.path().join("bundle.zip");
    {
        let file = std::fs::File::create(&zip_path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        // The hostile case: a symlink whose target is an absolute path OUTSIDE any
        // extraction root. Extraction must write these bytes as a plain file, never
        // materialize a link to `/etc/passwd`.
        writer.add_symlink("link", "/etc/passwd", options).expect("add symlink");
        writer.finish().expect("finish zip");
    }

    let parent: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Parent").with_local_fs_access());
    let source: Arc<dyn Volume> = Arc::new(ArchiveVolume::new(
        parent,
        zip_path.clone(),
        ArchiveFormat::Zip,
        VolumeHost::detached(),
    ));

    // A real-filesystem destination so we can stat the landed entry's kind.
    let dst_dir = tempfile::tempdir().expect("dst tempdir");
    let dest: Arc<dyn Volume> = Arc::new(LocalPosixVolume::local_folder(
        "Dest",
        dst_dir.path().to_str().unwrap(),
        crate::host::testing::pane_order_host(),
    ));

    let events = Arc::new(CollectorEventSink::new());
    let state = make_state(&engine);
    let config = VolumeCopyConfig {
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };

    let result = copy_volumes_with_progress(
        events.clone(),
        "symlink-extract-op",
        &state,
        Arc::clone(&source),
        &[zip_path.join("link")],
        Arc::clone(&dest),
        Path::new("/"),
        &config,
    )
    .await;
    assert!(result.is_ok(), "symlink extraction should succeed: {result:?}");

    // (a) The destination is a REGULAR file, never a symlink.
    let landed = dst_dir.path().join("link");
    let meta = std::fs::symlink_metadata(&landed).expect("dest entry exists");
    assert!(
        meta.file_type().is_file(),
        "extracted symlink entry must be a regular file, not a symlink"
    );
    assert!(
        !meta.file_type().is_symlink(),
        "must NOT be a symlink — that would be Zip Slip through the back door"
    );
    // (b) Its content is the target-path bytes, written verbatim (never followed).
    assert_eq!(std::fs::read(&landed).expect("read dest"), b"/etc/passwd");
}

// ========================================================================
// Gatekeeper's quarantine: a zip downloaded by a browser carries
// `com.apple.quarantine`, and `ditto -x`, bsdtar, and Archive Utility copy it
// onto every file AND folder they extract, so Gatekeeper still checks an app
// that came out of it. An extract that drops it hands the user an unchecked app.
// ========================================================================

#[cfg(target_os = "macos")]
mod quarantine {
    use super::*;
    use crate::transfer::volume::forward_volume_methods;
    use cmdr_fs::volume::StreamWritten;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::AtomicUsize;

    /// The quarantine `path` wears, or `None`.
    fn quarantine_of(path: &Path) -> Option<Vec<u8>> {
        use cmdr_fs::testing::metadata_fixture::QUARANTINE_XATTR;
        xattr::get(path, QUARANTINE_XATTR).unwrap_or_else(|e| panic!("read the quarantine of {}: {e}", path.display()))
    }

    /// An `ArchiveVolume` over `archive` on a local-backed parent, the way the app
    /// opens a `.zip` or `.tar.gz` that sits on a local drive. The parent's lane is
    /// unique, so a managed op (the move-out) never queues behind another test's.
    fn local_archive_volume(archive: &Path, format: cmdr_archive::ArchiveFormat) -> Arc<dyn Volume> {
        use cmdr_archive::ArchiveVolume;
        use cmdr_fs::volume::host::VolumeHost;
        let parent: Arc<dyn Volume> = Arc::new(
            InMemoryVolume::new("Parent")
                .with_local_fs_access()
                .with_lane_key(format!("m2-quarantine-{}", uuid::Uuid::new_v4())),
        );
        Arc::new(ArchiveVolume::new(
            parent,
            archive.to_path_buf(),
            format,
            VolumeHost::detached(),
        ))
    }

    /// A zip with a top-level file, an explicit folder, and an IMPLICIT one (no
    /// entry of its own, only a member's path names it), which is what a zip made by
    /// most tools other than Finder looks like.
    fn build_quarantine_zip(dir: &Path) -> PathBuf {
        use std::io::Write;
        let zip_path = dir.join("download.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).expect("create zip"));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("readme.txt", options).expect("start readme");
        writer.write_all(b"hello").expect("write readme");
        writer.add_directory("docs/", options).expect("add docs dir");
        writer.start_file("docs/a.txt", options).expect("start a");
        writer.write_all(b"aaa").expect("write a");
        writer.start_file("docs/implicit/c.txt", options).expect("start c");
        writer.write_all(b"ccc").expect("write c");
        writer.finish().expect("finish zip");
        zip_path
    }

    /// A gzip-compressed tar (the SEQUENTIAL extract path) of `files`.
    fn build_quarantine_targz(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
        use std::io::Write;
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut header = tar::Header::new_ustar();
            header.set_mtime(1_700_000_000);
            header.set_mode(0o644);
            header.set_size(data.len() as u64);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).expect("append file");
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&builder.into_inner().expect("finish tar"))
            .expect("gz write");
        let path = dir.join("download.tar.gz");
        std::fs::write(&path, gz.finish().expect("gz finish")).expect("write the tar.gz");
        path
    }

    async fn copy_out(
        source: &Arc<dyn Volume>,
        sources: &[PathBuf],
        dest: &Arc<dyn Volume>,
        state: &Arc<WriteOperationState>,
    ) {
        let result = copy_volumes_with_progress(
            Arc::new(CollectorEventSink::new()),
            "extract-out-quarantine-op",
            state,
            Arc::clone(source),
            sources,
            Arc::clone(dest),
            Path::new("/"),
            &VolumeCopyConfig {
                progress_interval_ms: 0,
                ..VolumeCopyConfig::default()
            },
        )
        .await;
        assert!(result.is_ok(), "extract-out should succeed: {result:?}");
    }

    fn local_dest(dir: &Path) -> Arc<dyn Volume> {
        Arc::new(LocalPosixVolume::local_folder(
            "Dest",
            dir.to_str().expect("dest path"),
            crate::host::testing::pane_order_host(),
        ))
    }

    /// Files and folders, the implicit folder included, carry the archive's value
    /// byte for byte, whether the item was selected on its own (the top-level
    /// dispatch) or found inside a folder (the merge walker).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn extracting_a_quarantined_zip_quarantines_every_file_and_folder() {
        use cmdr_fs::testing::metadata_fixture::{QUARANTINE_VALUE, QUARANTINE_XATTR, set_xattr};

        let engine = TestOperations::new();

        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = build_quarantine_zip(tmp.path());
        set_xattr(&zip_path, QUARANTINE_XATTR, QUARANTINE_VALUE);
        let dest_dir = tempfile::tempdir().expect("dest tempdir");

        let source = local_archive_volume(&zip_path, cmdr_archive::ArchiveFormat::Zip);
        let sources = [zip_path.join("readme.txt"), zip_path.join("docs")];
        copy_out(&source, &sources, &local_dest(dest_dir.path()), &make_state(&engine)).await;

        for landed in [
            "readme.txt",
            "docs",
            "docs/a.txt",
            "docs/implicit",
            "docs/implicit/c.txt",
        ] {
            assert_eq!(
                quarantine_of(&dest_dir.path().join(landed)).as_deref(),
                Some(QUARANTINE_VALUE),
                "{landed} came out of a quarantined zip, so it carries the zip's quarantine"
            );
        }
        assert_eq!(
            quarantine_of(dest_dir.path()),
            None,
            "the folder the user extracted INTO isn't something the extract made"
        );
    }

    /// A folder the extract merged into is the user's, and keeps its own metadata;
    /// only what the extract created inside it gets the quarantine.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_folder_the_extract_merged_into_stays_unquarantined() {
        use cmdr_fs::testing::metadata_fixture::{QUARANTINE_VALUE, QUARANTINE_XATTR, set_xattr};

        let engine = TestOperations::new();

        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = build_quarantine_zip(tmp.path());
        set_xattr(&zip_path, QUARANTINE_XATTR, QUARANTINE_VALUE);
        let dest_dir = tempfile::tempdir().expect("dest tempdir");
        std::fs::create_dir(dest_dir.path().join("docs")).expect("the user's own docs folder");

        let source = local_archive_volume(&zip_path, cmdr_archive::ArchiveFormat::Zip);
        copy_out(
            &source,
            &[zip_path.join("docs")],
            &local_dest(dest_dir.path()),
            &make_state(&engine),
        )
        .await;

        assert_eq!(
            quarantine_of(&dest_dir.path().join("docs")),
            None,
            "the user's folder keeps its own metadata"
        );
        for landed in ["docs/a.txt", "docs/implicit", "docs/implicit/c.txt"] {
            assert_eq!(
                quarantine_of(&dest_dir.path().join(landed)).as_deref(),
                Some(QUARANTINE_VALUE),
                "{landed} is new, so it carries the quarantine"
            );
        }
    }

    /// Nothing to propagate means nothing added.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn extracting_an_unquarantined_zip_adds_no_quarantine() {
        let engine = TestOperations::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = build_quarantine_zip(tmp.path());
        let dest_dir = tempfile::tempdir().expect("dest tempdir");

        let source = local_archive_volume(&zip_path, cmdr_archive::ArchiveFormat::Zip);
        let sources = [zip_path.join("readme.txt"), zip_path.join("docs")];
        copy_out(&source, &sources, &local_dest(dest_dir.path()), &make_state(&engine)).await;

        for landed in [
            "readme.txt",
            "docs",
            "docs/a.txt",
            "docs/implicit",
            "docs/implicit/c.txt",
        ] {
            assert_eq!(
                quarantine_of(&dest_dir.path().join(landed)),
                None,
                "{landed} has no quarantine to inherit"
            );
        }
    }

    /// A compressed tar extracts in one decode pass (`sequential_extract.rs`), its
    /// own write path; a member selected on its own still streams through the
    /// top-level dispatch. Both carry the quarantine.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn extracting_a_quarantined_tar_gz_quarantines_every_file_and_folder() {
        use cmdr_fs::testing::metadata_fixture::{QUARANTINE_VALUE, QUARANTINE_XATTR, set_xattr};

        let engine = TestOperations::new();

        let tmp = tempfile::tempdir().expect("tempdir");
        let archive = build_quarantine_targz(
            tmp.path(),
            &[
                ("top.txt", b"top"),
                ("docs/a.txt", b"alpha"),
                ("docs/sub/b.txt", b"bravo"),
            ],
        );
        set_xattr(&archive, QUARANTINE_XATTR, QUARANTINE_VALUE);
        let dest_dir = tempfile::tempdir().expect("dest tempdir");

        let source = local_archive_volume(&archive, cmdr_archive::ArchiveFormat::Tar(cmdr_archive::TarCodec::Gzip));
        assert!(
            source.extraction_is_sequential(&archive.join("docs")),
            "the fixture must take the one-pass path"
        );
        let sources = [archive.join("top.txt"), archive.join("docs")];
        copy_out(&source, &sources, &local_dest(dest_dir.path()), &make_state(&engine)).await;

        for landed in ["top.txt", "docs", "docs/a.txt", "docs/sub", "docs/sub/b.txt"] {
            assert_eq!(
                quarantine_of(&dest_dir.path().join(landed)).as_deref(),
                Some(QUARANTINE_VALUE),
                "{landed} came out of a quarantined tar.gz, so it carries its quarantine"
            );
        }
    }

    /// A destination that stops the operation the moment its first file lands.
    struct StopAfterFirstWrite {
        inner: Arc<LocalPosixVolume>,
        state: Arc<WriteOperationState>,
        writes: AtomicUsize,
    }

    impl Volume for StopAfterFirstWrite {
        forward_volume_methods!(inner =>
            name, root, list_directory, get_metadata, exists, is_directory, create_file, create_directory,
            create_directory_all, delete, rename, get_space_info, supports_streaming, supports_export,
            operations_are_local, max_concurrent_ops, scan_for_copy, open_read_stream, supports_unknown_length_writes,
            write_is_single_shot, create_directory_errors_on_existing_dir, local_path, supports_local_fs_access,
            lane_key, apply_metadata, metadata_caps,
        );

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn write_from_stream<'a>(
            &'a self,
            dest: &'a Path,
            mode: cmdr_fs::volume::WriteMode,
            length: cmdr_fs::volume::StreamLength,
            stream: Box<dyn cmdr_fs::volume::VolumeReadStream>,
            on_progress: &'a (dyn Fn(cmdr_fs::volume::StreamWriteProgress) -> std::ops::ControlFlow<()> + Sync),
        ) -> Pin<Box<dyn Future<Output = Result<StreamWritten, VolumeError>> + Send + 'a>> {
            use crate::state::OperationIntent;
            use std::sync::atomic::Ordering;
            Box::pin(async move {
                let written = self
                    .inner
                    .write_from_stream(dest, mode, length, stream, on_progress)
                    .await?;
                if self.writes.fetch_add(1, Ordering::SeqCst) == 0 {
                    self.state.intent().set_for_test(OperationIntent::Stopped);
                }
                Ok(written)
            })
        }
    }

    /// A cancelled extract keeps the folders it made (Cancel keeps what landed), and
    /// those folders are exactly what an unquarantined `.app` bundle would be: the
    /// quarantine goes on as each folder is created, never in a pass that a cancel
    /// skips. The one-pass extractor creates every folder before its first byte, so
    /// here the deepest one holds nothing at all when the cancel lands.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancelled_extract_leaves_the_folders_it_made_quarantined() {
        use cmdr_fs::testing::metadata_fixture::{QUARANTINE_VALUE, QUARANTINE_XATTR, set_xattr};

        let engine = TestOperations::new();

        let tmp = tempfile::tempdir().expect("tempdir");
        let archive = build_quarantine_targz(
            tmp.path(),
            &[("App/a.txt", b"alpha"), ("App/Contents/MacOS/run", b"#!/bin/sh\n")],
        );
        set_xattr(&archive, QUARANTINE_XATTR, QUARANTINE_VALUE);
        let dest_dir = tempfile::tempdir().expect("dest tempdir");

        let state = make_state(&engine);
        let dest: Arc<dyn Volume> = Arc::new(StopAfterFirstWrite {
            inner: Arc::new(LocalPosixVolume::local_folder(
                "Dest",
                dest_dir.path().to_str().expect("dest path"),
                crate::host::testing::pane_order_host(),
            )),
            state: Arc::clone(&state),
            writes: AtomicUsize::new(0),
        });
        let source = local_archive_volume(&archive, cmdr_archive::ArchiveFormat::Tar(cmdr_archive::TarCodec::Gzip));
        let _cancelled = copy_volumes_with_progress(
            Arc::new(CollectorEventSink::new()),
            "extract-out-quarantine-cancel-op",
            &state,
            source,
            &[archive.join("App")],
            dest,
            Path::new("/"),
            &VolumeCopyConfig {
                progress_interval_ms: 0,
                ..VolumeCopyConfig::default()
            },
        )
        .await;

        assert!(
            !dest_dir.path().join("App/Contents/MacOS/run").exists(),
            "the cancel landed before the second member"
        );
        for folder in ["App", "App/Contents", "App/Contents/MacOS"] {
            assert_eq!(
                quarantine_of(&dest_dir.path().join(folder)).as_deref(),
                Some(QUARANTINE_VALUE),
                "{folder} survived the cancel, so it carries the quarantine"
            );
        }
    }

    /// A move out of a zip extracts through the same copy engine before it deletes
    /// the entries from the archive, so it carries the quarantine too.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn moving_out_of_a_quarantined_zip_quarantines_what_lands() {
        use crate::event_sinks::OperationEventSink;
        use cmdr_fs::ignore_poison::IgnorePoison;
        use cmdr_fs::testing::metadata_fixture::{QUARANTINE_VALUE, QUARANTINE_XATTR, set_xattr};

        let engine = TestOperations::new();

        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = build_quarantine_zip(tmp.path());
        set_xattr(&zip_path, QUARANTINE_XATTR, QUARANTINE_VALUE);
        let dest_dir = tempfile::tempdir().expect("dest tempdir");

        let events = Arc::new(CollectorEventSink::new());
        engine
            .route_archive_move_out(
                Arc::clone(&events) as Arc<dyn OperationEventSink>,
                "root".to_string(),
                local_archive_volume(&zip_path, cmdr_archive::ArchiveFormat::Zip),
                vec![zip_path.join("readme.txt"), zip_path.join("docs")],
                "dest".to_string(),
                local_dest(dest_dir.path()),
                dest_dir.path().to_path_buf(),
                VolumeCopyConfig {
                    progress_interval_ms: 0,
                    ..VolumeCopyConfig::default()
                },
            )
            .await
            .expect("start the move-out");
        cmdr_fs::testing::wait_until_async(Duration::from_secs(10), "the move-out to complete", || {
            !events.complete.lock_ignore_poison().is_empty()
        })
        .await;

        for landed in [
            "readme.txt",
            "docs",
            "docs/a.txt",
            "docs/implicit",
            "docs/implicit/c.txt",
        ] {
            assert_eq!(
                quarantine_of(&dest_dir.path().join(landed)).as_deref(),
                Some(QUARANTINE_VALUE),
                "{landed} was moved out of a quarantined zip, so it carries the zip's quarantine"
            );
        }
    }
}
