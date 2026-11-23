//! What each walker does with a listing row's own path (`FileEntry.path`), read through
//! `listed_name.rs::listed_path`: a `..` row is refused, ❌ never walked, and a name
//! holding `\` (APFS's, SFTP's, or MTP's one-name `foo\..`) is a plain name on every
//! host's POSIX volume. An absolute row outside the listed folder still passes as it
//! always did (#1444).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::cleanup::{TreeRemoval, remove_tree};
use super::copy::copy_volumes_with_progress;
use super::r#move::move_volumes_with_progress;
use super::rename_merge_test_support::make_state;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::{VolumeCopyConfig, WriteOperationError};
use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{BackendKind, InMemoryVolume, ListingProgress, Volume, VolumeError};

/// A climbing row and an out-of-folder one, for the child really at `/album/evil.txt`.
const CLIMBING: &str = "/album/../victim.txt";

/// An in-memory volume whose listing of `folder` reports `reported` as the path of the row
/// really at `real`, from its `lie_from`-th listing of that folder on (0: every listing).
/// Its listings never mark a link, as Samba's don't, so `hidden_links` asks about folders.
struct LyingRows {
    inner: InMemoryVolume,
    folder: &'static str,
    real: &'static str,
    reported: &'static str,
    lie_from: usize,
    listings: AtomicUsize,
}

impl LyingRows {
    fn new(inner: InMemoryVolume, folder: &'static str, real: &'static str, reported: &'static str) -> Self {
        Self {
            inner,
            folder,
            real,
            reported,
            lie_from: 0,
            listings: AtomicUsize::new(0),
        }
    }
}

impl Volume for LyingRows {
    crate::forward_volume_methods!(inner => name, root, get_metadata, exists, is_directory, create_directory,
        create_file, delete, rename, get_space_info, supports_streaming, scan_for_copy, open_read_stream,
        write_from_stream);
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Smb
    }
    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = self.inner.list_directory(path, on_progress).await?;
            if path.as_str() == self.folder && self.listings.fetch_add(1, Ordering::SeqCst) >= self.lie_from {
                for entry in &mut entries {
                    if entry.path == self.real {
                        entry.path = self.reported.to_string();
                    }
                }
            }
            Ok(entries)
        })
    }
}

fn config() -> VolumeCopyConfig {
    VolumeCopyConfig {
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    }
}

/// `/album/evil.txt`, `/album/keep.txt`, and `/victim.txt` beside the album.
async fn album() -> InMemoryVolume {
    let volume = InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000);
    volume.create_directory(app_path!("/album")).await.unwrap();
    volume.create_file(app_path!("/album/evil.txt"), b"EVIL").await.unwrap();
    volume.create_file(app_path!("/album/keep.txt"), b"KEEP").await.unwrap();
    volume.create_file(app_path!("/victim.txt"), b"VICTIM").await.unwrap();
    volume
}

async fn inbox() -> Arc<InMemoryVolume> {
    let dest = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    dest.create_directory(app_path!("/inbox")).await.unwrap();
    dest
}

async fn read(volume: &dyn Volume, path: &str) -> Option<Vec<u8>> {
    let mut stream = volume
        .open_read_stream(AppPath::from_normalized(path).expect("canonical"))
        .await
        .ok()?;
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        out.extend_from_slice(&chunk.ok()?);
    }
    Some(out)
}

/// The merge refuses a climbing row as that child's name, and nothing lands for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_merge_refuses_a_climbing_row() {
    let engine = TestOperations::new();
    let source = Arc::new(LyingRows::new(album().await, "/album", "/album/evil.txt", CLIMBING));
    let dest = inbox().await;
    let result = copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "listed-row-merge",
        &make_state(&engine),
        Arc::clone(&source) as Arc<dyn Volume>,
        &[PathBuf::from("/album")],
        Arc::clone(&dest) as Arc<dyn Volume>,
        Path::new("/inbox"),
        &config(),
    )
    .await;
    let failure = result.expect_err("a climbing row must fail the copy");
    assert!(
        matches!(failure.error, WriteOperationError::InvalidName { .. }),
        "refused as a name, got {:?}",
        failure.error
    );
    assert_eq!(
        read(&*dest, "/inbox/victim.txt").await,
        None,
        "nothing of the climb lands"
    );
    assert_eq!(read(&*dest, "/victim.txt").await, None, "nothing of the climb lands");
}

/// An absolute row outside the listed folder still passes, as it always did: the merge
/// reads the row's own path. The gap a real "under its folder" check closes (#1444).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_merge_reads_an_out_of_folder_row_as_listed() {
    let engine = TestOperations::new();
    let source = Arc::new(LyingRows::new(
        album().await,
        "/album",
        "/album/evil.txt",
        "/victim.txt",
    ));
    let dest = inbox().await;
    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "listed-row-merge-outside",
        &make_state(&engine),
        Arc::clone(&source) as Arc<dyn Volume>,
        &[PathBuf::from("/album")],
        Arc::clone(&dest) as Arc<dyn Volume>,
        Path::new("/inbox"),
        &config(),
    )
    .await
    .expect("the copy runs");
    assert_eq!(
        read(&*dest, "/inbox/album/evil.txt").await.as_deref(),
        Some(&b"VICTIM"[..]),
        "today's behavior, pinned until #1444"
    );
    assert_eq!(read(&*source, "/victim.txt").await.as_deref(), Some(&b"VICTIM"[..]));
}

/// The move's sweep keeps a row that climbs on ITS listing (the copy's was honest): the
/// row's file stays, so does its folder, and nothing outside it goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_move_sweep_keeps_a_climbing_row() {
    let engine = TestOperations::new();
    let mut lying = LyingRows::new(album().await, "/album", "/album/evil.txt", CLIMBING);
    lying.lie_from = 1;
    let source = Arc::new(lying);
    let dest = inbox().await;
    let _ = move_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "listed-row-sweep",
        &make_state(&engine),
        Arc::clone(&source) as Arc<dyn Volume>,
        &[PathBuf::from("/album")],
        Arc::clone(&dest) as Arc<dyn Volume>,
        Path::new("/inbox"),
        &config(),
    )
    .await;
    assert!(
        source.listings.load(Ordering::SeqCst) >= 2,
        "the sweep listed the album, so this cell proves something"
    );
    assert_eq!(
        read(&*dest, "/inbox/album/evil.txt").await.as_deref(),
        Some(&b"EVIL"[..])
    );
    assert_eq!(read(&*source, "/victim.txt").await.as_deref(), Some(&b"VICTIM"[..]));
    assert_eq!(
        read(&*source, "/album/evil.txt").await.as_deref(),
        Some(&b"EVIL"[..]),
        "the row the sweep couldn't vouch for stays"
    );
    assert!(source.exists(app_path!("/album")).await, "and so does its folder");
}

/// `remove_tree` refuses a climbing row and keeps the folder holding it.
#[tokio::test]
async fn remove_tree_refuses_a_climbing_row() {
    let volume: Arc<dyn Volume> = Arc::new(LyingRows::new(album().await, "/album", "/album/evil.txt", CLIMBING));
    let failure = remove_tree(&volume, app_path!("/album"), TreeRemoval::UserChoseOverwriteAcrossTypes)
        .await
        .expect_err("a climbing row stops the removal");
    assert!(
        matches!(failure.error, VolumeError::InvalidName(_)),
        "{:?}",
        failure.error
    );
    assert!(
        volume.exists(app_path!("/victim.txt")).await,
        "nothing outside the tree goes"
    );
    assert!(volume.exists(app_path!("/album")).await, "the folder holding it stays");
}

/// `hidden_links` refuses a climbing folder row instead of asking about the place it names.
#[tokio::test]
async fn hidden_links_refuses_a_climbing_folder_row() {
    let inner = album().await;
    inner.create_directory(app_path!("/album/sub")).await.unwrap();
    let volume = LyingRows::new(inner, "/album", "/album/sub", "/album/../elsewhere");
    let result = crate::hidden_links::list_marking_hidden_links(&volume, Path::new("/album")).await;
    assert!(matches!(result, Err(VolumeError::InvalidName(_))), "{result:?}");
}

/// A name holding `\` is one name on a POSIX volume: a folder holding `foo\..` and
/// `..\bar` copies and moves whole, and the move's sweep takes them along. macOS and Linux
/// only: on Windows the leaf bodies past the walk still take `std::path`, so such a name is
/// the destination refusing it, asked per item (`native_join_refuses`, the plan's 10a), until
/// they take app paths. The walk's own reading is pinned on every host in `cmdr-fs`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_backslash_name_copies_and_moves_as_one_name() {
    let engine = TestOperations::new();
    for moving in [false, true] {
        let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
        source.create_directory(app_path!("/album")).await.unwrap();
        source.create_file(app_path!("/album/foo\\.."), b"ONE").await.unwrap();
        source.create_file(app_path!("/album/..\\bar"), b"TWO").await.unwrap();
        let dest = inbox().await;
        let (events, state) = (Arc::new(CollectorEventSink::new()), make_state(&engine));
        let (src, dst) = (
            Arc::clone(&source) as Arc<dyn Volume>,
            Arc::clone(&dest) as Arc<dyn Volume>,
        );
        let sources = [PathBuf::from("/album")];
        let result = if moving {
            move_volumes_with_progress(
                events,
                "bs-move",
                &state,
                src,
                &sources,
                dst,
                Path::new("/inbox"),
                &config(),
            )
            .await
        } else {
            copy_volumes_with_progress(
                events,
                "bs-copy",
                &state,
                src,
                &sources,
                dst,
                Path::new("/inbox"),
                &config(),
            )
            .await
        };
        assert!(result.is_ok(), "moving={moving}: {result:?}");
        assert_eq!(read(&*dest, "/inbox/album/foo\\..").await.as_deref(), Some(&b"ONE"[..]));
        assert_eq!(read(&*dest, "/inbox/album/..\\bar").await.as_deref(), Some(&b"TWO"[..]));
        assert_eq!(
            !source.exists(app_path!("/album/foo\\..")).await,
            moving,
            "moving={moving}"
        );
    }
}
