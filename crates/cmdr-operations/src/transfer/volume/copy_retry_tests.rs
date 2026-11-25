//! What a per-file retry (`retry.rs`) must not disturb, driven through the real
//! `copy_volumes_with_progress` pipeline.
//!
//! The retry itself is pinned in `volume/strategy_retry_tests.rs`. These cover
//! the operation-level invariants a retry sits inside, because they are the ones
//! whose failure mode is silent: a merge that quietly loses a dest-only file, a
//! finished copy that leaves a failed attempt's temp behind, and a transfer that
//! asks the user the same question twice.

use super::tests::make_state;
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::ConflictResolution;
use cmdr_fs::app_path;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{
    DirectoryCreation, InMemoryVolume, ListingProgress, ScanConflict, SourceItemInfo, SpaceInfo, StreamLength,
    StreamWriteProgress, Volume, VolumeReadStream,
};
use std::path::Path;
use std::pin::Pin as StdPin;

use super::super::super::conflict_responder_test_support::{ConflictResponderSink, file_conflict_count};
use cmdr_fs::volume::{StreamWritten, WriteMode};

/// A destination that fails the first `fail_writes` writes of ONE named file and
/// otherwise behaves exactly like the `InMemoryVolume` it wraps.
///
/// Wrapping (rather than owning) the inner volume is what lets a test set up a
/// pre-existing dest tree and then read it back through the same handle, which is
/// how the merge invariant is checked. The file is named by the prefix of its
/// final name, because a staged write targets `<name>.cmdr-tmp-<uuid>`.
struct FlakyMergeDest {
    inner: Arc<InMemoryVolume>,
    fail_writes: usize,
    error: VolumeError,
    name: String,
    calls: AtomicUsize,
    /// How many more `.cmdr-tmp-*` deletes to turn down, as a share does while
    /// the failed attempt's handle is still open.
    temp_deletes_to_refuse: AtomicUsize,
}

impl FlakyMergeDest {
    fn wrap(inner: Arc<InMemoryVolume>, fail_writes: usize, error: VolumeError, name: &str) -> Arc<Self> {
        Arc::new(Self {
            inner,
            fail_writes,
            error,
            name: name.to_owned(),
            calls: AtomicUsize::new(0),
            temp_deletes_to_refuse: AtomicUsize::new(0),
        })
    }

    /// How many writes of the named file were attempted. A test asserts on this
    /// so it can't pass by never having triggered the blip at all.
    fn write_calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Volume for FlakyMergeDest {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn root(&self) -> &AppPath {
        self.inner.root()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    fn max_concurrent_ops(&self) -> usize {
        self.inner.max_concurrent_ops()
    }
    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> StdPin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        self.inner.list_directory(path, on_progress)
    }
    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> StdPin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        self.inner.get_metadata(path)
    }
    fn exists<'a>(&'a self, path: &'a AppPath) -> StdPin<Box<dyn Future<Output = bool> + Send + 'a>> {
        self.inner.exists(path)
    }
    fn is_directory<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> StdPin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        self.inner.is_directory(path)
    }
    fn create_directory<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> StdPin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        self.inner.create_directory(path)
    }
    fn create_directory_all<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> StdPin<Box<dyn Future<Output = Result<DirectoryCreation, VolumeError>> + Send + 'a>> {
        self.inner.create_directory_all(path)
    }
    fn create_file<'a>(
        &'a self,
        path: &'a AppPath,
        content: &'a [u8],
    ) -> StdPin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        self.inner.create_file(path, content)
    }
    fn delete<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> StdPin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let path_app = path;
        let path_std = path.as_std_legacy();
        let path: &Path = &path_std;
        let is_temp = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains(".cmdr-tmp-"));
        let refused = is_temp
            && self
                .temp_deletes_to_refuse
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
                .is_ok();
        if refused {
            return Box::pin(async {
                Err(VolumeError::IoError {
                    message: "sharing violation".into(),
                    raw_os_error: None,
                })
            });
        }
        self.inner.delete(path_app)
    }
    fn rename<'a>(
        &'a self,
        from: &'a AppPath,
        to: &'a AppPath,
        force: bool,
    ) -> StdPin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        self.inner.rename(from, to, force)
    }
    fn get_space_info<'a>(&'a self) -> StdPin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        self.inner.get_space_info()
    }
    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> StdPin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        self.inner.open_read_stream(path)
    }
    fn create_directory_errors_on_existing_dir(&self) -> bool {
        self.inner.create_directory_errors_on_existing_dir()
    }
    fn scan_for_conflicts<'a>(
        &'a self,
        source_items: &'a [SourceItemInfo],
        dest_path: &'a AppPath,
    ) -> StdPin<Box<dyn Future<Output = Result<Vec<ScanConflict>, VolumeError>> + Send + 'a>> {
        self.inner.scan_for_conflicts(source_items, dest_path)
    }
    fn write_from_stream<'a>(
        &'a self,
        dest: &'a AppPath,
        mode: WriteMode,
        length: StreamLength,
        mut stream: Box<dyn VolumeReadStream>,
        on_progress: &'a (dyn Fn(StreamWriteProgress) -> std::ops::ControlFlow<()> + Sync),
    ) -> StdPin<Box<dyn Future<Output = Result<StreamWritten, VolumeError>> + Send + 'a>> {
        let dest_app = dest;
        let dest_std = dest.as_std_legacy();
        let dest: &Path = &dest_std;
        let Some(size) = length.known() else {
            return Box::pin(async { Err(VolumeError::NotSupported) });
        };
        let eligible = dest
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(self.name.as_str()));
        let attempt = if eligible {
            self.calls.fetch_add(1, Ordering::SeqCst)
        } else {
            usize::MAX
        };
        Box::pin(async move {
            if attempt >= self.fail_writes {
                return self
                    .inner
                    .write_from_stream(dest_app, mode, StreamLength::Known(size), stream, on_progress)
                    .await;
            }
            // Leave the partial behind, like a backend that never reached its own
            // cleanup: the staging layer is what has to clear it.
            let mut partial = Vec::new();
            if let Some(Ok(chunk)) = stream.next_chunk().await {
                partial.extend_from_slice(&chunk);
            }
            let _ = self.inner.delete(dest_app).await;
            let _ = self.inner.create_file(dest_app, &partial).await;
            Err(self.error.clone())
        })
    }
}

/// A source `/album` and a pre-existing dest `/album` that share one clashing
/// file and each keep one of their own.
async fn merge_fixture() -> (Arc<dyn Volume>, Arc<InMemoryVolume>) {
    let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    source.create_directory(app_path!("/album")).await.unwrap();
    source
        .create_file(app_path!("/album/fresh.txt"), b"SRC-fresh")
        .await
        .unwrap();
    source
        .create_file(app_path!("/album/clash.txt"), b"SRC-clash")
        .await
        .unwrap();

    let dest_inner = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    dest_inner.create_directory(app_path!("/album")).await.unwrap();
    dest_inner
        .create_file(app_path!("/album/keep.txt"), b"DEST-keep")
        .await
        .unwrap();
    dest_inner
        .create_file(app_path!("/album/clash.txt"), b"DEST-clash")
        .await
        .unwrap();

    (source as Arc<dyn Volume>, dest_inner)
}

async fn read_all(vol: &Arc<InMemoryVolume>, path: &str) -> Option<Vec<u8>> {
    let mut stream = vol
        .open_read_stream(AppPath::from_normalized(path).expect("a canonical path"))
        .await
        .ok()?;
    let mut out = Vec::new();
    while let Some(Ok(chunk)) = stream.next_chunk().await {
        out.extend_from_slice(&chunk);
    }
    Some(out)
}

/// The merge invariant across an attempt boundary: a merge never deletes or
/// overwrites a dest file the source doesn't shadow, and a retry must not turn
/// one into a replace. The retried child re-runs its write; nothing about the
/// LEVEL is re-walked, re-created, or re-deleted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retried_merge_child_still_leaves_dest_only_files_alone() {
    let engine = TestOperations::new();
    let (source, dest_inner) = merge_fixture().await;
    let state = make_state(&engine);
    let op = engine.register_state("retry-merge", Arc::clone(&state));
    // Only the source-only file blips, and only once.
    let flaky = FlakyMergeDest::wrap(
        Arc::clone(&dest_inner),
        1,
        VolumeError::ConnectionTimeout("send timed out".into()),
        "fresh.txt",
    );
    let dest: Arc<dyn Volume> = Arc::clone(&flaky) as Arc<dyn Volume>;

    let events = Arc::new(CollectorEventSink::new());
    let config = VolumeCopyConfig {
        conflict_resolution: ConflictResolution::Overwrite,
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };

    copy_volumes_with_progress(
        events,
        op.id().as_str(),
        &state,
        Arc::clone(&source),
        &crate::testing::app_paths_of(&[PathBuf::from("/album")]),
        Arc::clone(&dest),
        &crate::testing::app_path_of(Path::new("/")),
        &config,
    )
    .await
    .expect("a blip on one merge child must not fail the merge");

    assert_eq!(
        read_all(&dest_inner, "/album/keep.txt").await.as_deref(),
        Some(&b"DEST-keep"[..]),
        "the dest-only file must survive a retried merge untouched"
    );
    assert_eq!(
        flaky.write_calls(),
        2,
        "the fixture must actually have blipped, or this test proves nothing"
    );
    assert_eq!(
        read_all(&dest_inner, "/album/fresh.txt").await.as_deref(),
        Some(&b"SRC-fresh"[..]),
        "the retried child must arrive whole"
    );
    assert_eq!(
        read_all(&dest_inner, "/album/clash.txt").await.as_deref(),
        Some(&b"SRC-clash"[..]),
        "the shadowed file follows the policy, exactly as without the blip"
    );
}

/// ❗ A finished copy still owes the cleanup a stopped one gets: a retried file's
/// first temp outlives its attempt when the share turns its delete down
/// (`StagedWrite::abandon` keeps it listed for the post-loop), and a copy whose
/// files then all landed must not walk away from it. The sweep takes that temp
/// and ❌ nothing else: every landed file and the dest-only file stay whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_copy_removes_the_temp_a_retried_file_could_not_delete() {
    let engine = TestOperations::new();
    let (source, dest_inner) = merge_fixture().await;
    let state = make_state(&engine);
    let op = engine.register_state("retry-refused-temp", Arc::clone(&state));
    let flaky = FlakyMergeDest::wrap(
        Arc::clone(&dest_inner),
        1,
        VolumeError::ConnectionTimeout("send timed out".into()),
        "fresh.txt",
    );
    flaky.temp_deletes_to_refuse.store(1, Ordering::SeqCst);
    let dest: Arc<dyn Volume> = Arc::clone(&flaky) as Arc<dyn Volume>;
    let config = VolumeCopyConfig {
        conflict_resolution: ConflictResolution::Overwrite,
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        op.id().as_str(),
        &state,
        Arc::clone(&source),
        &crate::testing::app_paths_of(&[PathBuf::from("/album")]),
        Arc::clone(&dest),
        &crate::testing::app_path_of(Path::new("/")),
        &config,
    )
    .await
    .expect("a blip on one file must not fail the copy");
    engine.leftover_cleanups_settled().await;

    assert_eq!(flaky.write_calls(), 2, "the fixture must actually have blipped");
    assert_eq!(
        flaky.temp_deletes_to_refuse.load(Ordering::SeqCst),
        0,
        "the attempt's delete must actually have been refused"
    );
    let mut names: Vec<String> = dest_inner
        .list_directory(app_path!("/album"), None)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    assert_eq!(names, ["clash.txt", "fresh.txt", "keep.txt"], "no temp stays behind");
    assert!(state.in_flight_temps.lock_ignore_poison().is_empty());
    for (path, content) in [
        ("/album/clash.txt", &b"SRC-clash"[..]),
        ("/album/fresh.txt", &b"SRC-fresh"[..]),
        ("/album/keep.txt", &b"DEST-keep"[..]),
    ] {
        assert_eq!(read_all(&dest_inner, path).await.as_deref(), Some(content), "{path}");
    }
}

/// A retry must not ask the user again. Conflict resolution runs on the driver,
/// ABOVE the retry loop, so a file that is run again re-runs only its write —
/// carrying the decision the user already made, and never re-emitting the prompt.
///
/// The other half of the same decision: the answer is not stale either. It was
/// given for this file, in this operation, seconds ago; re-asking would be the
/// surprising behavior, not the safe one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retried_file_never_re_asks_the_conflict_the_user_already_answered() {
    let engine = TestOperations::new();
    let (source, dest_inner) = merge_fixture().await;
    let state = make_state(&engine);
    let op = engine.register_state("retry-no-reprompt", Arc::clone(&state));
    // The CLASHING file blips, so the retry happens after its prompt was answered.
    let flaky = FlakyMergeDest::wrap(
        Arc::clone(&dest_inner),
        1,
        VolumeError::ConnectionTimeout("send timed out".into()),
        "clash.txt",
    );
    let dest: Arc<dyn Volume> = Arc::clone(&flaky) as Arc<dyn Volume>;

    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Overwrite, false));
    let config = VolumeCopyConfig {
        conflict_resolution: ConflictResolution::Stop,
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };

    copy_volumes_with_progress(
        events.clone(),
        op.id().as_str(),
        &state,
        Arc::clone(&source),
        &crate::testing::app_paths_of(&[PathBuf::from("/album")]),
        Arc::clone(&dest),
        &crate::testing::app_path_of(Path::new("/")),
        &config,
    )
    .await
    .expect("a blip on the clashing child must not fail the merge");

    assert_eq!(
        file_conflict_count(&events.inner),
        1,
        "the one clash must be asked about exactly once, however many attempts its write took"
    );
    assert_eq!(
        flaky.write_calls(),
        2,
        "the fixture must actually have blipped, or this test proves nothing"
    );
    assert_eq!(
        read_all(&dest_inner, "/album/clash.txt").await.as_deref(),
        Some(&b"SRC-clash"[..]),
        "the answered Overwrite must still be what lands"
    );
    assert_eq!(
        read_all(&dest_inner, "/album/keep.txt").await.as_deref(),
        Some(&b"DEST-keep"[..]),
        "and the dest-only file is still not the source's to touch"
    );
}
