//! One file of a snapshot as a [`VolumeReadStream`]: bounded chunks, each one
//! `read_file_at` on a blocking thread, [`READ_AHEAD`] of them in flight.

use crate::hub::{IndexSource, RepoSlot};
use crate::path::text_of;
use crate::repo::{IndexedRepo, ResticError, worker_crashed};
use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::metadata::{CarriedMetadata, MetadataClasses};
use cmdr_fs::volume::{StreamLength, VolumeError, VolumeReadStream};
use rustic_core::vfs::OpenFile;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

/// How much one chunk reads. The house streaming size, and a 1 MiB read spans a
/// handful of blobs.
pub(crate) const CHUNK_SIZE: usize = 1024 * 1024;

/// How many chunks a stream reads ahead of the one handed out. Each read waits
/// a round trip for its blobs, so one at a time leaves the link idle: four
/// took copy-out over direct SMB from ~21 to ~39 MB/s, and eight did no better
/// (`docs/notes/restic/open-cost.md` § "Reading blobs ahead"). Bounds a
/// stream's memory at four chunks.
pub(crate) const READ_AHEAD: usize = 4;

/// One chunk's read, running on the blocking pool.
type Pending = JoinHandle<Result<Vec<u8>, ResticError>>;

/// A snapshot file's bytes, read in [`CHUNK_SIZE`] windows, handed out in
/// order.
///
/// ❗ Dropping the stream (a Cancel) DETACHES the reads in flight, ❌ never
/// aborts them: a blocking read can't be cut, and each one finishes its parent
/// round trip whole, so a parent mid-transaction (a phone) is never left
/// half-way. At most [`READ_AHEAD`] of them run on after a drop.
pub(crate) struct ResticReadStream {
    pub(crate) repo: Arc<IndexedRepo>,
    /// Where [`Self::repo`] came from, to load it again when a pack it names is
    /// gone ([`ResticError::PackGone`]: a prune elsewhere, mid-copy).
    pub(crate) source: IndexSource,
    /// The stream reloaded its index once already: a pack gone from a fresh
    /// index is a damaged repo, ❌ never another reload.
    pub(crate) reloaded: bool,
    pub(crate) file: Arc<OpenFile>,
    pub(crate) runtime: Handle,
    /// The path asked for, for the error a short read answers.
    pub(crate) path: AppPathBuf,
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
    /// The node's permission bits (see `tree::permissions_of`).
    pub(crate) mode: u32,
    /// The repo, when the file was opened under `latest/`: every chunk counts as
    /// a read there, so `latest` stays put for the whole copy.
    pub(crate) latest_of: Option<Arc<RepoSlot>>,
    pub(crate) read: u64,
    /// The chunks in flight, oldest first, each with the length it asked for.
    pub(crate) ahead: VecDeque<(Pending, usize)>,
    /// Where the next chunk to start reading begins.
    pub(crate) queued_to: u64,
}

impl ResticReadStream {
    /// Starts chunk reads until [`READ_AHEAD`] are in flight or the file's end
    /// is queued.
    fn fill(&mut self) {
        while self.ahead.len() < READ_AHEAD && self.queued_to < self.size {
            let want = usize::try_from(self.size - self.queued_to).map_or(CHUNK_SIZE, |left| left.min(CHUNK_SIZE));
            let offset = usize::try_from(self.queued_to).unwrap_or(usize::MAX);
            let (repo, file) = (Arc::clone(&self.repo), Arc::clone(&self.file));
            let read = self.runtime.spawn_blocking(move || {
                repo.repo
                    .read_file_at(&file, offset, want)
                    .map(|bytes| bytes.to_vec())
                    .map_err(|err| repo.classify(&err))
            });
            self.ahead.push_back((read, want));
            self.queued_to += want as u64;
        }
    }

    /// After a chunk that didn't come back whole: detaches the reads behind it,
    /// so a caller asking again starts over at the chunk that failed.
    fn forget_ahead(&mut self) {
        self.ahead.clear();
        self.queued_to = self.read;
    }

    /// The next chunk in order, reading ahead.
    async fn next_read(&mut self) -> Option<(Result<Vec<u8>, ResticError>, usize)> {
        self.fill();
        let (read, want) = self.ahead.pop_front()?;
        Some((read.await.map_err(worker_crashed).and_then(|chunk| chunk), want))
    }

    /// Swaps in a fresh index after `err`, when it's the stream's first pack
    /// gone (`Ok(true)`: read again). The reads in flight, all through the
    /// stale index, are detached first.
    async fn reload_after(&mut self, err: &ResticError) -> Result<bool, VolumeError> {
        if !matches!(err, ResticError::PackGone(_)) || self.reloaded {
            return Ok(false);
        }
        self.forget_ahead();
        self.reloaded = true;
        self.repo = self.source.reload(&self.repo, None).await?;
        Ok(true)
    }

    /// `len` bytes at `offset` (fewer only at the file's end), reloading a stale
    /// index once like a chunk does.
    pub(crate) async fn read_at(&mut self, offset: usize, len: usize) -> Result<Vec<u8>, VolumeError> {
        loop {
            match read_at(&self.runtime, &self.repo, &self.file, offset, len).await {
                Err(err) if self.reload_after(&err).await? => {}
                read => return read.map_err(|err| err.into_volume_error(&self.path)),
            }
        }
    }
}

impl VolumeReadStream for ResticReadStream {
    fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
        Box::pin(async move {
            if self.read >= self.size {
                return None;
            }
            if let Some(slot) = &self.latest_of {
                slot.touch_latest();
            }
            let (mut chunk, mut want) = self.next_read().await?;
            if let Err(err) = &chunk {
                match self.reload_after(err).await {
                    Ok(true) => (chunk, want) = self.next_read().await?,
                    Ok(false) => {}
                    Err(err) => return Some(Err(err)),
                }
            }
            let chunk = chunk.map_err(|err| err.into_volume_error(&self.path));
            Some(match chunk {
                // A chunk that comes back short means the file ends before its
                // recorded size: a damaged snapshot. The chunks after it were
                // asked for at offsets that assume it was whole, and feeding a
                // short file to a copy would land it silently.
                Ok(bytes) if bytes.len() != want => {
                    self.forget_ahead();
                    Err(VolumeError::SourceChanged(text_of(&self.path)))
                }
                Ok(bytes) => {
                    self.read += bytes.len() as u64;
                    Ok(bytes)
                }
                Err(err) => {
                    self.forget_ahead();
                    Err(err)
                }
            })
        })
    }

    fn total_size(&self) -> StreamLength {
        StreamLength::Known(self.size)
    }

    fn bytes_read(&self) -> u64 {
        self.read
    }

    fn modified_at(&self) -> Option<SystemTime> {
        self.modified
    }

    /// The node's permission bits and nothing else: rustic's nodes carry no
    /// creation date, and restoring flags or xattrs isn't this view's job.
    fn metadata(&self, wanted: MetadataClasses) -> CarriedMetadata {
        CarriedMetadata {
            mode: Some(self.mode),
            ..CarriedMetadata::default()
        }
        .restricted_to(wanted)
    }
}

/// Reads `len` bytes of `file` at `offset` on a blocking thread. Fewer only at
/// the end of the file.
async fn read_at(
    runtime: &Handle,
    repo: &Arc<IndexedRepo>,
    file: &Arc<OpenFile>,
    offset: usize,
    len: usize,
) -> Result<Vec<u8>, ResticError> {
    let (repo, file) = (Arc::clone(repo), Arc::clone(file));
    runtime
        .spawn_blocking(move || {
            repo.repo
                .read_file_at(&file, offset, len)
                .map(|bytes| bytes.to_vec())
                .map_err(|err| repo.classify(&err))
        })
        .await
        .map_err(worker_crashed)?
}

#[cfg(test)]
#[path = "read_stream_tests.rs"]
mod tests;
