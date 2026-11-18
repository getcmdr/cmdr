//! Bytes from a volume as a `Read + Seek`: [`VolumeByteSource`] bridges a
//! blocking parser to the async `Volume::read_range`, and [`BlockReader`] turns
//! any [`ByteSource`] into a seekable reader that fetches 64 KiB blocks on demand.

use cmdr_fs::app_path::AppPath;
use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cmdr_fs::deadline::{Deadline, timeout_detached_within};
use cmdr_fs::log_error::LogError;
use cmdr_fs::volume::{Volume, VolumeError};

use crate::ReadError;

/// The most bytes one file's read may fetch. Past it the read stops with
/// `ReadError::TooLarge`: metadata never needs more, and a slow volume shouldn't
/// pay for a file that hides its tags behind megabytes.
pub const MAX_FETCHED_BYTES_PER_FILE: u64 = 4 * 1024 * 1024;

/// What [`BlockReader`] fetches at once. Big enough that a header walk is one or
/// two round trips on a NAS, small enough that skipping a cover costs little.
const BLOCK_SIZE: u64 = 64 * 1024;

/// How many fetched blocks [`BlockReader`] keeps. Parsers jump between a header
/// and a few boxes or frames, so a handful covers their back-and-forth.
const KEPT_BLOCKS: usize = 8;

/// Positioned reads of one file.
pub trait ByteSource: Send + Sync {
    /// The file's size in bytes.
    fn size(&self) -> u64;
    /// Up to `len` bytes from `offset`; fewer only at the end of the file.
    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>>;
}

/// One file on a volume, read through `Volume::read_range`. ❌ Its `read_at`
/// blocks on the runtime, so it must only run on a blocking thread
/// (`spawn_blocking`), never on the async executor.
pub struct VolumeByteSource {
    volume: Arc<dyn Volume>,
    path: PathBuf,
    size: u64,
    handle: tokio::runtime::Handle,
    deadline: Deadline,
    cancel: Arc<AtomicBool>,
}

impl VolumeByteSource {
    /// The file at `path` on `volume`, `size` bytes long (from the listing). Reads
    /// run on `handle` within `deadline` and stop once `cancel` is set.
    pub fn new(
        volume: Arc<dyn Volume>,
        path: PathBuf,
        size: u64,
        handle: tokio::runtime::Handle,
        deadline: Deadline,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            volume,
            path,
            size,
            handle,
            deadline,
            cancel,
        }
    }
}

impl ByteSource for VolumeByteSource {
    fn size(&self) -> u64 {
        self.size
    }

    /// Checks the cancel flag and the deadline before asking the volume, so a
    /// closed sheet or a spent budget costs no further round trip. The range
    /// read runs detached under what's left of the deadline: on a timeout it
    /// finishes on its own, which keeps an MTP transaction whole.
    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(ReadError::Cancelled.into_io());
        }
        if len == 0 || offset >= self.size {
            return Ok(Vec::new());
        }
        let want = len.min(usize::try_from(self.size - offset).unwrap_or(usize::MAX));
        let volume = Arc::clone(&self.volume);
        let path = self.path.clone();
        self.handle
            .block_on(timeout_detached_within(
                &self.deadline,
                || ReadError::TimedOut,
                ReadError::Io,
                async move {
                    volume
                        .read_range(&AppPath::legacy_unchecked(&path), offset, want)
                        .await
                        .map_err(|error| match error {
                            VolumeError::NotSupported => ReadError::NotSupported,
                            other => {
                                // Logged here, marked: `Io`'s text must carry no path.
                                log::debug!("A media range read refused: {}", LogError(&other));
                                ReadError::Io(match other.raw_os_error() {
                                    Some(errno) => format!("the volume refused a range read (os error {errno})"),
                                    None => "the volume refused a range read".to_string(),
                                })
                            }
                        })
                },
            ))
            .map_err(ReadError::into_io)
    }
}

/// A `Read + Seek` over a [`ByteSource`]: 64 KiB blocks fetched on demand, a few
/// kept, at most [`MAX_FETCHED_BYTES_PER_FILE`] fetched, and holes served as
/// zeros without fetching.
pub struct BlockReader<S: ByteSource> {
    source: S,
    size: u64,
    position: u64,
    /// Sorted by start, never overlapping.
    holes: Vec<Range<u64>>,
    /// Kept blocks by their start offset, the most recently used last.
    blocks: VecDeque<(u64, Vec<u8>)>,
    fetched: u64,
}

impl<S: ByteSource> BlockReader<S> {
    /// A reader at the start of `source`.
    pub fn new(source: S) -> Self {
        let size = source.size();
        Self {
            source,
            size,
            position: 0,
            holes: Vec::new(),
            blocks: VecDeque::with_capacity(KEPT_BLOCKS),
            fetched: 0,
        }
    }

    /// The same reader, serving the bytes in `holes` as zeros without fetching
    /// them (cover art a tag parser reads only to skip). Ranges may come in any
    /// order and may overlap.
    pub fn with_holes(mut self, mut holes: Vec<Range<u64>>) -> Self {
        holes.retain(|hole| hole.start < hole.end);
        holes.sort_by_key(|hole| hole.start);
        let mut merged: Vec<Range<u64>> = Vec::with_capacity(holes.len());
        for hole in holes {
            match merged.last_mut() {
                Some(last) if hole.start <= last.end => last.end = last.end.max(hole.end),
                _ => merged.push(hole),
            }
        }
        self.holes = merged;
        self
    }

    /// The hole `offset` sits in, if any.
    fn hole_at(&self, offset: u64) -> Option<&Range<u64>> {
        let after = self.holes.partition_point(|hole| hole.start <= offset);
        self.holes[..after].last().filter(|hole| offset < hole.end)
    }

    /// Where the first hole after `offset` starts, or the end of the file.
    fn next_hole_start(&self, offset: u64) -> u64 {
        let after = self.holes.partition_point(|hole| hole.start <= offset);
        self.holes
            .get(after)
            .map_or(self.size, |hole| hole.start.min(self.size))
    }

    /// The kept block starting at `start`, fetched first if it isn't kept.
    fn block(&mut self, start: u64) -> io::Result<&[u8]> {
        if let Some(index) = self.blocks.iter().position(|(kept, _)| *kept == start) {
            let block = self.blocks.remove(index).expect("the index came from this deque");
            self.blocks.push_back(block);
        } else {
            let want = BLOCK_SIZE.min(self.size - start);
            if self.fetched + want > MAX_FETCHED_BYTES_PER_FILE {
                return Err(ReadError::TooLarge.into_io());
            }
            let data = self.fetch(start, want)?;
            if self.blocks.len() == KEPT_BLOCKS {
                self.blocks.pop_front();
            }
            self.blocks.push_back((start, data));
        }
        Ok(&self.blocks.back().expect("a block was just pushed").1)
    }

    /// `want` bytes from `start`, asking again after a short answer until the
    /// source says the file ends (an empty answer).
    fn fetch(&mut self, start: u64, want: u64) -> io::Result<Vec<u8>> {
        let want = usize::try_from(want).expect("a block fits in memory");
        let mut data = Vec::with_capacity(want);
        while data.len() < want {
            let chunk = self.source.read_at(start + data.len() as u64, want - data.len())?;
            if chunk.is_empty() {
                break;
            }
            self.fetched += chunk.len() as u64;
            data.extend_from_slice(&chunk);
        }
        data.truncate(want);
        Ok(data)
    }
}

impl<S: ByteSource> Read for BlockReader<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.position >= self.size {
            return Ok(0);
        }
        let position = self.position;
        let room = buf.len() as u64;
        let count = if let Some(hole) = self.hole_at(position) {
            let count = room.min(hole.end.min(self.size) - position) as usize;
            buf[..count].fill(0);
            count
        } else {
            let start = position - position % BLOCK_SIZE;
            let stop = self.next_hole_start(position).min(start + BLOCK_SIZE);
            let block = self.block(start)?;
            let from = (position - start) as usize;
            // A block shorter than asked means the file ended early (it shrank).
            let until = ((stop - start) as usize).min(block.len());
            if from >= until {
                return Ok(0);
            }
            let count = (until - from).min(buf.len());
            buf[..count].copy_from_slice(&block[from..from + count]);
            count
        };
        self.position += count as u64;
        Ok(count)
    }
}

impl<S: ByteSource> Seek for BlockReader<S> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        let Some(target) = target else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the file",
            ));
        };
        self.position = target;
        Ok(target)
    }
}
