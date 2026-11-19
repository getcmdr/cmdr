//! The source a cancel cell holds still: one file, handed out a chunk at a time.
//!
//! ❗ This, not a big file and a stopwatch, is what makes "the cancel landed
//! while the upload was still running" a FACT. Grant one permit and the transfer
//! is parked at a known byte offset, with the destination holding an open,
//! incomplete staging sibling, for as long as the cell needs. It also keeps the
//! cell inside the workspace-wide 8 s nextest cap, which a payload big enough to
//! outrun a stopwatch would not.
//!
//! Same shape as `transfer/volume/copy_wedge_test_support.rs`'s `GatedChunkStream`,
//! which is private to that module and so can't be reused from out here. It lives
//! apart from `network_transfer_test_support.rs` because the `Volume` impl below
//! is mostly trait signatures, and folding them in put that file over the
//! `file-length` warn threshold.

use cmdr_fs::app_path::AppPath;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cmdr_fs::volume::Volume;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{CopyScanResult, InMemoryVolume, ListingProgress, VolumeError, VolumeReadStream};
use cmdr_operations::testing::forward_volume_methods;

/// How big the file a cancel interrupts is, and how much of it one permit buys.
///
/// Small on purpose: the gate is what makes the cancel land mid-file, so nothing
/// here has to be big enough to outrun a timer.
pub(super) const CANCEL_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;
const CANCEL_CHUNK_BYTES: usize = 64 * 1024;

/// A read stream that hands out one chunk per permit from a test-owned
/// semaphore.
struct GatedChunks {
    bytes: Arc<Vec<u8>>,
    emitted: usize,
    gate: Arc<tokio::sync::Semaphore>,
    handed_out: Arc<AtomicU64>,
}

impl VolumeReadStream for GatedChunks {
    fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
        Box::pin(async move {
            if self.emitted >= self.bytes.len() {
                return None;
            }
            match self.gate.acquire().await {
                Ok(permit) => permit.forget(),
                Err(_) => return None,
            }
            let end = (self.emitted + CANCEL_CHUNK_BYTES).min(self.bytes.len());
            let chunk = self.bytes[self.emitted..end].to_vec();
            self.emitted = end;
            self.handed_out.fetch_add(1, Ordering::SeqCst);
            Some(Ok(chunk))
        })
    }

    fn total_size(&self) -> cmdr_fs::volume::StreamLength {
        cmdr_fs::volume::StreamLength::Known(self.bytes.len() as u64)
    }

    fn bytes_read(&self) -> u64 {
        self.emitted as u64
    }

    fn modified_at(&self) -> Option<std::time::SystemTime> {
        None
    }
}

/// A source volume whose files' bytes only move when the cell says so, every
/// file drawing on one gate. Everything but the read stream is an
/// `InMemoryVolume`'s answer.
struct GatedUploadSource {
    inner: InMemoryVolume,
    files: HashMap<PathBuf, Arc<Vec<u8>>>,
    gate: Arc<tokio::sync::Semaphore>,
    handed_out: Arc<AtomicU64>,
    /// The one file whose read fails, once its cue gets a permit. A source
    /// with one reads several files at once (`max_concurrent_ops`), so a copy
    /// runs its concurrent driver, the one that has siblings to stop; the
    /// others answer the trait's one-at-a-time default.
    failing: Option<(PathBuf, Arc<tokio::sync::Semaphore>)>,
}

/// The stream of the file whose read fails on cue: nothing until the cue, then
/// a refusal (an `IoError` with no errno, which no retry or outage wait takes).
struct FailsOnCue {
    len: u64,
    cue: Arc<tokio::sync::Semaphore>,
}

impl VolumeReadStream for FailsOnCue {
    fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
        Box::pin(async move {
            if let Ok(permit) = self.cue.acquire().await {
                permit.forget();
            }
            Some(Err(VolumeError::IoError {
                message: "Injected read failure".into(),
                raw_os_error: None,
            }))
        })
    }

    fn total_size(&self) -> cmdr_fs::volume::StreamLength {
        cmdr_fs::volume::StreamLength::Known(self.len)
    }

    fn bytes_read(&self) -> u64 {
        0
    }

    fn modified_at(&self) -> Option<std::time::SystemTime> {
        None
    }
}

impl Volume for GatedUploadSource {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn root(&self) -> &AppPath {
        self.inner.root()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn supports_export(&self) -> bool {
        true
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    fn max_concurrent_ops(&self) -> usize {
        if self.failing.is_some() { 8 } else { 1 }
    }
    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        self.inner.list_directory(path, on_progress)
    }
    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        self.inner.get_metadata(path)
    }
    fn exists<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        self.inner.exists(path)
    }
    fn is_directory<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        self.inner.is_directory(path)
    }
    fn scan_for_copy<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<CopyScanResult, VolumeError>> + Send + 'a>> {
        self.inner.scan_for_copy(path)
    }
    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        let path_app = path;
        let path_std = path.as_std_legacy();
        let path: &Path = &path_std;
        let bytes = self.files.get(path).map(Arc::clone);
        let gate = Arc::clone(&self.gate);
        let handed_out = Arc::clone(&self.handed_out);
        let cue = self
            .failing
            .as_ref()
            .filter(|(failing, _)| failing == path)
            .map(|(_, cue)| Arc::clone(cue));
        Box::pin(async move {
            let path_std = path_app.as_std_legacy();
            let path: &Path = &path_std;
            let bytes = bytes.ok_or_else(|| VolumeError::NotFound(path.display().to_string()))?;
            let stream: Box<dyn VolumeReadStream> = match cue {
                Some(cue) => Box::new(FailsOnCue {
                    len: bytes.len() as u64,
                    cue,
                }),
                None => Box::new(GatedChunks {
                    bytes,
                    emitted: 0,
                    gate,
                    handed_out,
                }),
            };
            Ok(stream)
        })
    }
}

/// The handles a cancel cell drives the source through.
pub(super) struct GatedUpload {
    /// The source to copy FROM: one file at `/big.bin`, or a tree.
    pub(super) volume: Arc<dyn Volume>,
    /// One permit buys one chunk.
    pub(super) gate: Arc<tokio::sync::Semaphore>,
    /// Chunks handed to the destination so far, so a cell can wait on "the
    /// upload has actually started" with a plain synchronous condition.
    pub(super) handed_out: Arc<AtomicU64>,
}

/// Builds the source with its gate closed, so nothing moves until the cell says
/// so.
pub(super) async fn gated_upload(bytes: Vec<u8>) -> GatedUpload {
    gated_files(vec![("/big.bin".to_string(), bytes)]).await
}

/// [`gated_upload`] holding several files at absolute paths (`/tree/a.bin`),
/// their folders made, every one drawing on the same gate, so a cell can let
/// some files land whole and hold one mid-body.
pub(super) async fn gated_files(files: Vec<(String, Vec<u8>)>) -> GatedUpload {
    gated_files_failing(files, None).await
}

/// [`gated_files`] where reading `failing` fails once the returned cue gets a
/// permit, so a cell can hold one file's failure until its siblings are
/// mid-body.
pub(super) async fn gated_files_failing_at(
    files: Vec<(String, Vec<u8>)>,
    failing: &str,
) -> (GatedUpload, Arc<tokio::sync::Semaphore>) {
    let cue = Arc::new(tokio::sync::Semaphore::new(0));
    let upload = gated_files_failing(files, Some((PathBuf::from(failing), Arc::clone(&cue)))).await;
    (upload, cue)
}

async fn gated_files_failing(
    files: Vec<(String, Vec<u8>)>,
    failing: Option<(PathBuf, Arc<tokio::sync::Semaphore>)>,
) -> GatedUpload {
    let inner = InMemoryVolume::new("Gated");
    let mut served = HashMap::new();
    for (path, bytes) in files {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent().filter(|parent| *parent != Path::new("/")) {
            inner
                .create_directory_all(&AppPath::legacy_unchecked(parent))
                .await
                .expect("making the gated source's folders");
        }
        inner
            .create_file(&AppPath::legacy_unchecked(&path), &bytes)
            .await
            .expect("seeding the gated source");
        served.insert(path, Arc::new(bytes));
    }
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let handed_out = Arc::new(AtomicU64::new(0));
    GatedUpload {
        volume: Arc::new(GatedUploadSource {
            inner,
            files: served,
            gate: Arc::clone(&gate),
            handed_out: Arc::clone(&handed_out),
            failing,
        }),
        gate,
        handed_out,
    }
}

// ── Holding a LIVE server's reads still ──────────────────────────────

/// A live volume whose read streams hand out one chunk per permit, for a cell
/// that cancels a DOWNLOAD mid-file.
///
/// Everything but `open_read_stream` forwards to the server untouched, so the
/// walk, the metadata, and the bytes are all real; only their pace is the
/// cell's.
struct GatedReads {
    inner: Arc<dyn Volume>,
    gate: Arc<tokio::sync::Semaphore>,
    handed_out: Arc<AtomicU64>,
}

/// The server's own stream, released one chunk per permit.
struct GatedLiveStream {
    inner: Box<dyn VolumeReadStream>,
    gate: Arc<tokio::sync::Semaphore>,
    handed_out: Arc<AtomicU64>,
}

impl VolumeReadStream for GatedLiveStream {
    fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
        Box::pin(async move {
            match self.gate.acquire().await {
                Ok(permit) => permit.forget(),
                Err(_) => return None,
            }
            let chunk = self.inner.next_chunk().await;
            if matches!(chunk, Some(Ok(_))) {
                self.handed_out.fetch_add(1, Ordering::SeqCst);
            }
            chunk
        })
    }

    fn total_size(&self) -> cmdr_fs::volume::StreamLength {
        self.inner.total_size()
    }

    fn bytes_read(&self) -> u64 {
        self.inner.bytes_read()
    }

    fn modified_at(&self) -> Option<std::time::SystemTime> {
        self.inner.modified_at()
    }

    /// The source's own metadata, so a destination that stores it keeps it.
    fn metadata(&self, wanted: cmdr_fs::metadata::MetadataClasses) -> cmdr_fs::metadata::CarriedMetadata {
        self.inner.metadata(wanted)
    }
}

impl Volume for GatedReads {
    forward_volume_methods!(
        inner => name,
        root,
        lane_key,
        list_directory,
        get_metadata,
        exists,
        is_directory,
        create_file,
        create_directory,
        create_directory_all,
        delete,
        rename,
        get_space_info,
        local_path,
        supports_streaming,
        supports_export,
        supports_local_fs_access,
        operations_are_local,
        max_concurrent_ops,
        create_directory_errors_on_existing_dir,
        scan_for_copy,
        scan_for_copy_batch,
        scan_for_conflicts,
        write_from_stream,
        write_is_single_shot,
    );

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let inner = self.inner.open_read_stream(path).await?;
            let stream: Box<dyn VolumeReadStream> = Box::new(GatedLiveStream {
                inner,
                gate: Arc::clone(&self.gate),
                handed_out: Arc::clone(&self.handed_out),
            });
            Ok(stream)
        })
    }
}

/// `remote`, with its reads held until the cell grants permits. Same handles as
/// [`gated_upload`], with `volume` the wrapped server.
pub(super) fn gated_reads(remote: Arc<dyn Volume>) -> GatedUpload {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let handed_out = Arc::new(AtomicU64::new(0));
    GatedUpload {
        volume: Arc::new(GatedReads {
            inner: remote,
            gate: Arc::clone(&gate),
            handed_out: Arc::clone(&handed_out),
        }),
        gate,
        handed_out,
    }
}
