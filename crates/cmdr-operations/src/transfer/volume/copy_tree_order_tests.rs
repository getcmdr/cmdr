//! Every volume driver STARTS files in tree order: each folder's subfolders
//! first, then its files, by name, depth first, and the top-level sources the
//! same way whatever order the selection came in. It's the order the progress
//! tree lists its rows in, so the rows below the active one really are next.
//! `DETAILS.md` § "Processing order".
//!
//! The source here lists every level BACKWARDS, the way an SMB or MTP listing
//! comes back in no particular order, and records the order files are opened
//! for reading, which is the order they start.

use super::super::faulty_volume::forward_volume_methods;
use super::tests::make_state;
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::{InMemoryVolume, ListingProgress};

/// A remote-shaped source whose listings come back reversed and which records
/// every file it opens.
struct ReversedListingSource {
    inner: Arc<InMemoryVolume>,
    width: usize,
    opened: std::sync::Mutex<Vec<String>>,
}

impl ReversedListingSource {
    fn opened(&self) -> Vec<String> {
        self.opened.lock_ignore_poison().clone()
    }
}

impl Volume for ReversedListingSource {
    forward_volume_methods!(
        inner => name, root, lane_key, get_metadata, exists, is_directory, create_file, create_directory,
        create_directory_all, delete, rename, get_space_info, supports_streaming, supports_export,
        create_directory_errors_on_existing_dir, scan_for_copy, scan_for_copy_batch,
        supports_unknown_length_writes, write_from_stream, write_is_single_shot,
    );

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// A transport limit, so the width binds the operation's windows.
    fn operations_are_local(&self) -> bool {
        false
    }

    fn max_concurrent_ops(&self) -> usize {
        self.width
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = self.inner.list_directory(path, on_progress).await?;
            entries.reverse();
            Ok(entries)
        })
    }

    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn cmdr_fs::volume::VolumeReadStream>, VolumeError>> + Send + 'a>>
    {
        Box::pin(async move {
            let path_app = path;
            let path_std = path.as_std_legacy();
            let path: &Path = &path_std;
            self.opened.lock_ignore_poison().push(path.display().to_string());
            self.inner.open_read_stream(path_app).await
        })
    }
}

/// `/src` holding `a/` (`sub/mmm.txt`, `zzz.txt`), `b/aaa.txt`, `Y.txt`, and
/// `z.txt`, behind a reversed-listing source of the given width.
async fn tree_source(width: usize) -> Arc<ReversedListingSource> {
    let inner = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    for dir in ["/src", "/src/a", "/src/a/sub", "/src/b"] {
        inner
            .create_directory(AppPath::from_normalized(dir).expect("a canonical path"))
            .await
            .unwrap();
    }
    for file in [
        "/src/a/zzz.txt",
        "/src/a/sub/mmm.txt",
        "/src/b/aaa.txt",
        "/src/Y.txt",
        "/src/z.txt",
    ] {
        inner
            .create_file(AppPath::from_normalized(file).expect("a canonical path"), b"bytes")
            .await
            .unwrap();
    }
    Arc::new(ReversedListingSource {
        inner,
        width,
        opened: std::sync::Mutex::new(Vec::new()),
    })
}

/// The selection, out of order on purpose.
fn shuffled_selection() -> Vec<PathBuf> {
    ["/src/z.txt", "/src/b", "/src/Y.txt", "/src/a"]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

const TREE_ORDER: [&str; 5] = [
    "/src/a/sub/mmm.txt",
    "/src/a/zzz.txt",
    "/src/b/aaa.txt",
    "/src/Y.txt",
    "/src/z.txt",
];

fn dest() -> Arc<dyn Volume> {
    Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000))
}

/// One folder is one source, so the serial driver walks it; its merge walker
/// must start each level's files in tree order however the listing came back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_merge_whose_listing_comes_back_shuffled_still_starts_files_in_tree_order() {
    let engine = TestOperations::new();
    let source = tree_source(1).await;

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "tree-order-merge",
        &make_state(&engine),
        Arc::clone(&source) as Arc<dyn Volume>,
        &[PathBuf::from("/src/a")],
        dest(),
        Path::new("/out"),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("the copy succeeds");

    assert_eq!(source.opened(), ["/src/a/sub/mmm.txt", "/src/a/zzz.txt"]);
}

/// Fewer than three sources take the serial driver, one source at a time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_serial_driver_starts_its_sources_in_tree_order() {
    let engine = TestOperations::new();
    let source = tree_source(1).await;

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "tree-order-serial",
        &make_state(&engine),
        Arc::clone(&source) as Arc<dyn Volume>,
        &[PathBuf::from("/src/z.txt"), PathBuf::from("/src/a")],
        dest(),
        Path::new("/out"),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("the copy succeeds");

    assert_eq!(source.opened(), ["/src/a/sub/mmm.txt", "/src/a/zzz.txt", "/src/z.txt"]);
}

/// The concurrent driver spawns sources in tree order. With two in flight a
/// neighbor may open first, so each file starts at most one place from its
/// tree position, ❌ never at its selection position.
///
/// The tasks run on the ENGINE's runtime, which here is this test's single
/// thread, so they start in the order they were spawned. On a pool, two
/// walkers listing their folders in parallel can start in either order, and
/// what this pins is the spawn order, not the scheduler's.
#[tokio::test(flavor = "current_thread")]
async fn the_concurrent_driver_starts_its_sources_in_tree_order() {
    let engine = TestOperations::with(|builder| {
        builder.volume_host(
            cmdr_fs::volume::host::VolumeHost::builder()
                .runtime(tokio::runtime::Handle::current())
                .build(),
        )
    });
    let source = tree_source(2).await;

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "tree-order-concurrent",
        &make_state(&engine),
        Arc::clone(&source) as Arc<dyn Volume>,
        &shuffled_selection(),
        dest(),
        Path::new("/out"),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("the copy succeeds");

    let opened = source.opened();
    assert_eq!(opened.len(), TREE_ORDER.len(), "every file copied once: {opened:?}");
    for (started, path) in opened.iter().enumerate() {
        let expected = TREE_ORDER
            .iter()
            .position(|tree_path| tree_path == path)
            .expect("only selected files open");
        assert!(
            started.abs_diff(expected) <= 1,
            "{path} started {started}th, tree position {expected}: {opened:?}"
        );
    }
}

/// A cross-volume move copies through the same serial driver and walker.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_starts_files_in_tree_order() {
    let engine = TestOperations::new();
    let source = tree_source(1).await;

    super::super::move_cross::move_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "tree-order-move",
        &make_state(&engine),
        Arc::clone(&source) as Arc<dyn Volume>,
        &shuffled_selection(),
        dest(),
        Path::new("/out"),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("the move succeeds");

    assert_eq!(source.opened(), TREE_ORDER);
}
