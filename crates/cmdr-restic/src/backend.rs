//! The storage adapter: rustic's blocking `ReadBackend` over a Cmdr `Volume`.
//!
//! rustic reads a repo through a tiny, blocking storage trait. This implements it
//! on top of the volume the repo lives on, so a repo on an SMB share, SFTP, S3,
//! or a phone opens the same way a local one does. Every method `block_on`s the
//! parent's async call on the captured runtime handle, which is only sound off
//! the runtime's own workers: rustic is only ever driven from `spawn_blocking`
//! (`repo.rs`). Same bridge as `cmdr-archive`'s `VolumeByteSource`.

use crate::index_feed::{IndexFeed, Taken};
use bytes::Bytes;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::{Volume, VolumeError, VolumeRandomReader};
use rustic_core::repofile::KeyFile;
use rustic_core::{BytesList, ErrorKind, FileType, Id, ReadBackend, RusticError, RusticResult, WriteBackend};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

/// Reads a restic repo's files through the volume it lives on, and refuses every
/// write.
pub(crate) struct VolumeBackend {
    parent: Arc<dyn Volume>,
    /// The repo's folder on `parent`: the one holding `config`.
    root: PathBuf,
    handle: Handle,
    /// How many writes rustic tried. Every one is refused; the count is what lets
    /// a test pin "browsing never even tries to write".
    write_attempts: AtomicUsize,
    /// The thread each whole-file read ran on, by file type.
    #[cfg(test)]
    read_threads: Mutex<Vec<(FileType, String)>>,
    /// Packs held open for positioned reads (see [`HeldReaders`]). Shared with
    /// the sweep that closes them once idle.
    held: Arc<Mutex<HeldReaders>>,
    /// How long the held readers may sit unused before they close.
    reader_idle: Duration,
    /// During an index load: the index files read ahead, the load's cancel,
    /// and its byte tally (`index_load.rs`). `None` on every other repo.
    index_feed: Option<Arc<IndexFeed>>,
}

impl VolumeBackend {
    pub(crate) fn new(parent: Arc<dyn Volume>, root: PathBuf, handle: Handle) -> Self {
        Self {
            parent,
            root,
            handle,
            write_attempts: AtomicUsize::new(0),
            #[cfg(test)]
            read_threads: Mutex::new(Vec::new()),
            held: Arc::new(Mutex::new(HeldReaders::default())),
            reader_idle: READER_IDLE,
            index_feed: None,
        }
    }

    /// The adapter an index load opens its repo through: rustic lists and takes
    /// the index files from `feed`.
    pub(crate) fn with_index_feed(mut self, feed: Arc<IndexFeed>) -> Self {
        self.index_feed = Some(feed);
        self
    }

    /// The load's feed, when this adapter is an index load's.
    #[cfg(test)]
    pub(crate) fn index_feed(&self) -> Option<&Arc<IndexFeed>> {
        self.index_feed.as_ref()
    }

    /// The same adapter with a shorter reader idle, for a test that waits for it.
    #[cfg(test)]
    pub(crate) fn with_reader_idle(mut self, idle: Duration) -> Self {
        self.reader_idle = idle;
        self
    }

    /// The thread each whole-file read of type `tpe` ran on.
    #[cfg(test)]
    pub(crate) fn read_threads(&self, tpe: FileType) -> Vec<String> {
        lock(&self.read_threads)
            .iter()
            .filter(|(read, _)| *read == tpe)
            .map(|(_, thread)| thread.clone())
            .collect()
    }

    /// How many write calls rustic made (all refused).
    #[cfg(test)]
    pub(crate) fn write_attempts(&self) -> usize {
        self.write_attempts.load(Ordering::Relaxed)
    }

    /// Where a file of type `tpe` lives, per the restic repo layout. Packs fan out
    /// into 256 subfolders by their id's first two hex digits.
    fn path_of(&self, tpe: FileType, id: &Id) -> PathBuf {
        match tpe {
            FileType::Config => self.root.join("config"),
            FileType::Pack => {
                let hex = id.to_hex();
                self.root.join(tpe.dirname()).join(&hex[..2]).join(&*hex)
            }
            FileType::Index | FileType::Key | FileType::Snapshot => self.root.join(tpe.dirname()).join(&*id.to_hex()),
        }
    }

    /// The files in `dir` whose names are restic ids, with their sizes. A missing
    /// folder lists as empty (`index/` can be empty, and some copies drop it).
    fn list_ids_in(&self, dir: &Path) -> RusticResult<Vec<(Id, u32)>> {
        let entries = match self
            .handle
            .block_on(self.parent.list_directory(&AppPath::legacy_unchecked(dir), None))
        {
            Ok(entries) => entries,
            Err(VolumeError::NotFound(_)) => return Ok(Vec::new()),
            Err(err) => return Err(backend_error(err)),
        };
        Ok(entries
            .into_iter()
            .filter(|e| !e.is_directory)
            .filter_map(|e| {
                let id = e.name.parse::<Id>().ok()?;
                Some((id, size_u32(e.size.unwrap_or(0))))
            })
            .collect())
    }

    /// Streams a whole file. Used for the small files (config, keys, snapshots,
    /// index) and as the fallback for a parent without ranged reads.
    fn read_stream(&self, path: &Path, stop_after: Option<usize>) -> Result<Vec<u8>, VolumeError> {
        self.handle.block_on(async {
            let mut stream = self.parent.open_read_stream(&AppPath::legacy_unchecked(path)).await?;
            let mut out = Vec::new();
            while let Some(chunk) = stream.next_chunk().await {
                out.extend_from_slice(&chunk?);
                if stop_after.is_some_and(|limit| out.len() >= limit) {
                    break;
                }
            }
            Ok(out)
        })
    }
}

impl VolumeBackend {
    /// The key files rustic may try a password on: at most [`MAX_KEYS`] of them
    /// (the first by id), each a real key file whose scrypt cost is tolerable.
    ///
    /// rustic tries every key it lists, and passes each file's own `N`, `r`, and
    /// `p` (plaintext JSON) straight to scrypt with no ceiling. Anyone can leave
    /// a folder that looks like a repo on a shared drive, so without this a
    /// typed password could ask scrypt for terabytes (an allocation abort) or
    /// thousands of one-second derivations. rustic finds keys only through this
    /// listing, so a key skipped here is never derived.
    fn tolerable_keys(&self, mut keys: Vec<(Id, u32)>) -> RusticResult<Vec<(Id, u32)>> {
        keys.sort_unstable_by_key(|(id, _)| *id);
        keys.truncate(MAX_KEYS);
        let mut tolerable = Vec::with_capacity(keys.len());
        let mut summed_cost: u64 = 0;
        for (id, size) in keys {
            if size > MAX_KEY_FILE_BYTES {
                continue;
            }
            // Bounded by the limit too, not by the listing's size: a backend that
            // reports none lists every file as 0 bytes.
            let limit = MAX_KEY_FILE_BYTES as usize;
            let bytes = self
                .read_stream(&self.path_of(FileType::Key, &id), Some(limit + 1))
                .map_err(backend_error)?;
            if bytes.len() > limit {
                continue;
            }
            let Ok(key) = serde_json::from_slice::<KeyFile>(&bytes) else {
                continue;
            };
            let Some(cost) = scrypt_cost(key.n, key.r, key.p).filter(|cost| *cost <= MAX_SCRYPT_COST) else {
                continue;
            };
            // rustic tries the keys one after the other, so their costs add up.
            if summed_cost + cost > MAX_TOTAL_SCRYPT_COST {
                continue;
            }
            summed_cost += cost;
            tolerable.push((id, size));
        }
        Ok(tolerable)
    }
}

impl VolumeBackend {
    /// `[offset, offset + length)` of `path`. A pack is read through a reader
    /// held open for it when the parent hands those out; anything else (and a
    /// parent that doesn't) is one `read_range`.
    fn read_window(&self, tpe: FileType, path: &Path, offset: u64, length: usize) -> Result<Vec<u8>, VolumeError> {
        if tpe == FileType::Pack
            && let Some(reader) = self.held_reader(path)?
        {
            return self.handle.block_on(reader.read_at(offset, length));
        }
        self.handle
            .block_on(self.parent.read_range(&AppPath::legacy_unchecked(path), offset, length))
    }

    /// The reader held open for the pack at `path`, opened on first use. `None`
    /// when the parent has no positioned readers (it said `NotSupported` once,
    /// and isn't asked again).
    ///
    /// A pack is read at many offsets (a tree, then each blob of a file), and on
    /// SMB a `read_range` is an open, a read, and a close: ~14 ms a tree, against
    /// one round trip through a held handle (`docs/notes/restic/open-cost.md`).
    /// Packs never change once written, so a held handle never goes stale.
    fn held_reader(&self, path: &Path) -> Result<Option<Arc<HeldReader>>, VolumeError> {
        {
            let mut held = lock(&self.held);
            if held.unsupported {
                return Ok(None);
            }
            if let Some(reader) = held.take(path) {
                self.note_reader_use(&mut held);
                return Ok(Some(reader));
            }
        }
        let opened = match self
            .handle
            .block_on(self.parent.open_random_reader(&AppPath::legacy_unchecked(path)))
        {
            Ok(reader) => Arc::new(HeldReader {
                reader: Some(reader),
                handle: self.handle.clone(),
            }),
            Err(VolumeError::NotSupported) => {
                lock(&self.held).unsupported = true;
                return Ok(None);
            }
            Err(err) => return Err(err),
        };
        // Two reads of one new pack may both have opened it; the second's
        // reader is dropped (and closed) here.
        let mut held = lock(&self.held);
        let reader = held.take(path).unwrap_or(opened);
        held.put(path, Arc::clone(&reader));
        self.note_reader_use(&mut held);
        Ok(Some(reader))
    }

    /// Marks the held readers used just now, and starts the sweep that closes
    /// them once they've sat idle for `reader_idle`, unless one runs already.
    fn note_reader_use(&self, held: &mut HeldReaders) {
        held.last_used = Instant::now();
        if !held.sweeping {
            held.sweeping = true;
            drop(
                self.handle
                    .spawn(close_when_idle(Arc::downgrade(&self.held), self.reader_idle)),
            );
        }
    }
}

/// Closes every held reader once none was used for `idle`, then ends; the next
/// read starts a new sweep. ❗ Independent of the index's own idle drop: a held
/// handle keeps a deleted file delete-pending on Samba and Windows, so a repo's
/// `data/` couldn't be removed for minutes after browsing.
async fn close_when_idle(held: Weak<Mutex<HeldReaders>>, idle: Duration) {
    let mut wait = idle;
    loop {
        tokio::time::sleep(wait).await;
        let Some(held) = held.upgrade() else { return };
        let mut readers = lock(&held);
        let idle_for = readers.last_used.elapsed();
        if idle_for < idle {
            wait = idle - idle_for;
            continue;
        }
        readers.sweeping = false;
        let closing = std::mem::take(&mut readers.readers);
        drop(readers);
        // Each closes on drop, as its last user lets go.
        drop(closing);
        return;
    }
}

/// Packs held open for positioned reads, least recently used first, at most
/// [`MAX_HELD_READERS`]. Dropping the adapter (the repo going away) drops them
/// all, and each closes as its last user lets go ([`HeldReader`]'s `Drop`).
struct HeldReaders {
    readers: Vec<(PathBuf, Arc<HeldReader>)>,
    /// The parent answered `NotSupported`: every pack read is a `read_range`.
    unsupported: bool,
    /// When a held reader was last taken or put.
    last_used: Instant,
    /// A sweep that closes idle readers is running.
    sweeping: bool,
}

impl Default for HeldReaders {
    fn default() -> Self {
        Self {
            readers: Vec::new(),
            unsupported: false,
            last_used: Instant::now(),
            sweeping: false,
        }
    }
}

impl HeldReaders {
    /// The reader for `path`, moved to most recently used.
    fn take(&mut self, path: &Path) -> Option<Arc<HeldReader>> {
        let at = self.readers.iter().position(|(held, _)| held == path)?;
        let entry = self.readers.remove(at);
        let reader = Arc::clone(&entry.1);
        self.readers.push(entry);
        Some(reader)
    }

    /// Holds `reader` for `path`, letting go of the least recently used one past
    /// the cap. `path` must not be held already.
    fn put(&mut self, path: &Path, reader: Arc<HeldReader>) {
        if self.readers.iter().all(|(held, _)| held != path) {
            self.readers.push((path.to_path_buf(), reader));
        }
        if self.readers.len() > MAX_HELD_READERS {
            self.readers.remove(0);
        }
    }
}

/// One pack's open reader. Closes itself on drop, on the runtime, so a reader
/// let go mid-read (evicted while another thread reads through it) closes when
/// that read finishes, and no server keeps a handle until its session ends.
struct HeldReader {
    reader: Option<Box<dyn VolumeRandomReader>>,
    handle: Handle,
}

impl HeldReader {
    async fn read_at(&self, offset: u64, length: usize) -> Result<Vec<u8>, VolumeError> {
        match &self.reader {
            Some(reader) => reader.read_at(offset, length).await,
            None => Err(VolumeError::NotSupported),
        }
    }
}

impl Drop for HeldReader {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            // A failed close only costs the server a handle until the session
            // ends; there's no one left to tell.
            drop(self.handle.spawn(async move {
                let _ = reader.close().await;
            }));
        }
    }
}

/// Locks a std mutex, taking the data over from a thread that panicked holding
/// it: the reader list stays whole between statements.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The most key files offered to rustic. A real repo has a handful (one per
/// person or machine); a password typed into a crafted one with thousands would
/// otherwise cost a second of scrypt per key.
pub(crate) const MAX_KEYS: usize = 32;

/// How long held pack readers may sit unused before they close: long enough to
/// span the gaps of a browse or a copy, short enough that deleting the repo's
/// files right after browsing doesn't meet a delete-pending file.
const READER_IDLE: Duration = Duration::from_secs(5);

/// The most packs held open for positioned reads. A tree walk or a file read
/// touches a few packs at a time; a handful more keeps a copy's working set open
/// without holding a server-side handle per pack in the repo.
pub(crate) const MAX_HELD_READERS: usize = 8;

/// The largest file read as a key. A real one is about 450 bytes of JSON.
const MAX_KEY_FILE_BYTES: u32 = 64 * 1024;

/// The ceiling on a key's scrypt cost, `N·r·p`: 8 times rustic's own default
/// (`N = 2^17`, `r = 8`, `p = 1`) and 32 times restic's (`N = 2^15`). It also
/// caps scrypt's memory, `128·N·r` bytes, at 1 GiB.
const MAX_SCRYPT_COST: u64 = 1 << 23;

/// Whether deriving a key with these scrypt parameters stays inside
/// [`MAX_SCRYPT_COST`].
#[cfg(test)]
pub(crate) fn scrypt_cost_is_tolerable(n: u32, r: u32, p: u32) -> bool {
    scrypt_cost(n, r, p).is_some_and(|cost| cost <= MAX_SCRYPT_COST)
}

/// A key's scrypt cost, `N·r·p`, or `None` when it overflows.
fn scrypt_cost(n: u32, r: u32, p: u32) -> Option<u64> {
    u64::from(n)
        .checked_mul(u64::from(r))
        .and_then(|nr| nr.checked_mul(u64::from(p)))
}

/// The ceiling on the SUMMED cost of every key offered: rustic tries them one
/// after the other, so ten keys at [`MAX_SCRYPT_COST`] would be ten 1 GiB,
/// multi-second derivations per typed password. Four at the ceiling, or 32 at
/// rustic's default; a real repo has a handful of keys at the default.
pub(crate) const MAX_TOTAL_SCRYPT_COST: u64 = 4 * MAX_SCRYPT_COST;

/// A size as rustic's listing wants it. Pack files stay far below 4 GiB (rustic
/// caps them), so the clamp is a guard, never a real path.
fn size_u32(size: u64) -> u32 {
    u32::try_from(size).unwrap_or(u32::MAX)
}

/// Wraps a parent failure so it survives rustic's error plumbing as the error's
/// typed source; [`volume_error_in`] finds it again.
fn backend_error(err: VolumeError) -> Box<RusticError> {
    RusticError::with_source(
        ErrorKind::Backend,
        "Reading the repository through its volume didn't work.",
        err,
    )
}

/// A pack read failing: the parent's error, typed as [`PackGone`] when the
/// parent says the pack isn't there.
fn pack_error(tpe: FileType, err: VolumeError) -> Box<RusticError> {
    if tpe == FileType::Pack && matches!(err, VolumeError::NotFound(_)) {
        return RusticError::with_source(
            ErrorKind::Backend,
            "A pack file the index names is gone from the repository.",
            PackGone(err),
        );
    }
    backend_error(err)
}

/// A pack the index names that the parent says is gone: a prune elsewhere
/// repacked its blobs since the index was loaded (or the repo is damaged). Its
/// source is the parent's `NotFound`, so [`volume_error_in`] still finds it.
#[derive(Debug)]
pub(crate) struct PackGone(pub(crate) VolumeError);

impl std::fmt::Display for PackGone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a pack file is gone from the repository")
    }
}

impl std::error::Error for PackGone {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// The parent's [`VolumeError`] somewhere in `err`'s source chain, if a parent
/// failure is what caused it. A typed downcast, never a look at message text.
pub(crate) fn volume_error_in<'a>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a VolumeError> {
    in_chain(err)
}

/// The [`PackGone`] somewhere in `err`'s source chain, if a missing pack is
/// what caused it.
pub(crate) fn pack_gone_in<'a>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a PackGone> {
    in_chain(err)
}

fn in_chain<'a, T: std::error::Error + 'static>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a T> {
    let mut current = Some(err);
    while let Some(err) = current {
        if let Some(found) = err.downcast_ref::<T>() {
            return Some(found);
        }
        current = err.source();
    }
    None
}

/// The one refusal every write gets: browsing must never change a user's backup.
fn refuse_write(be: &VolumeBackend) -> Box<RusticError> {
    be.write_attempts.fetch_add(1, Ordering::Relaxed);
    RusticError::new(ErrorKind::Unsupported, "Cmdr opens restic repositories read-only.")
}

impl ReadBackend for VolumeBackend {
    fn location(&self) -> String {
        self.root.display().to_string()
    }

    fn list_with_size(&self, tpe: FileType) -> RusticResult<Vec<(Id, u32)>> {
        match tpe {
            // rustic lists the config as one file with the default id.
            FileType::Config => match self.handle.block_on(
                self.parent
                    .get_metadata(&AppPath::legacy_unchecked(&self.path_of(tpe, &Id::default()))),
            ) {
                Ok(entry) if !entry.is_directory => Ok(vec![(Id::default(), size_u32(entry.size.unwrap_or(0)))]),
                Ok(_) | Err(VolumeError::NotFound(_)) => Ok(Vec::new()),
                Err(err) => Err(backend_error(err)),
            },
            // `data/` holds up to 256 subfolders; list the ones that exist rather
            // than asking for all 256 names.
            FileType::Pack => {
                let data = self.root.join(tpe.dirname());
                let subdirs = match self
                    .handle
                    .block_on(self.parent.list_directory(&AppPath::legacy_unchecked(&data), None))
                {
                    Ok(entries) => entries,
                    Err(VolumeError::NotFound(_)) => return Ok(Vec::new()),
                    Err(err) => return Err(backend_error(err)),
                };
                let mut packs = Vec::new();
                for subdir in subdirs.into_iter().filter(|e| e.is_directory) {
                    packs.extend(self.list_ids_in(&data.join(&subdir.name))?);
                }
                Ok(packs)
            }
            FileType::Key => self.tolerable_keys(self.list_ids_in(&self.root.join(tpe.dirname()))?),
            // During an index load, the load's own listing: rustic asks for
            // exactly the files the feed streams, and `index/` is listed once.
            FileType::Index if let Some(feed) = self.index_feed.as_ref().filter(|feed| !feed.has_stepped_aside()) => {
                Ok(feed.listing())
            }
            FileType::Index | FileType::Snapshot => self.list_ids_in(&self.root.join(tpe.dirname())),
        }
    }

    fn read_full(&self, tpe: FileType, id: &Id) -> RusticResult<Bytes> {
        #[cfg(test)]
        lock(&self.read_threads).push((tpe, std::thread::current().name().unwrap_or_default().to_string()));
        if tpe == FileType::Index
            && let Some(feed) = &self.index_feed
        {
            match feed.take(id).map_err(backend_error)? {
                Taken::Bytes(bytes) => return Ok(bytes),
                Taken::NotFed => {
                    let bytes = self.read_stream(&self.path_of(tpe, id), None).map_err(backend_error)?;
                    feed.note_read(bytes.len());
                    return Ok(Bytes::from(bytes));
                }
            }
        }
        self.read_stream(&self.path_of(tpe, id), None)
            .map(Bytes::from)
            .map_err(|err| pack_error(tpe, err))
    }

    fn read_partial(&self, tpe: FileType, id: &Id, _cacheable: bool, offset: u32, length: u32) -> RusticResult<Bytes> {
        let path = self.path_of(tpe, id);
        let (offset, length) = (u64::from(offset), length as usize);
        let window = match self.read_window(tpe, &path, offset, length) {
            Ok(bytes) => bytes,
            // No positioned reads on this parent: stream from the start and cut
            // the window out. Correct, not cheap; every Cmdr backend that hosts
            // repos in practice has `read_range`.
            Err(VolumeError::NotSupported) => {
                let end = offset as usize + length;
                let whole = self.read_stream(&path, Some(end)).map_err(|err| pack_error(tpe, err))?;
                whole
                    .get(offset as usize..end.min(whole.len()))
                    .unwrap_or_default()
                    .to_vec()
            }
            Err(err) => return Err(pack_error(tpe, err)),
        };
        if window.len() != length {
            return Err(RusticError::new(
                ErrorKind::InputOutput,
                "A repository file ended before the part rustic asked for.",
            ));
        }
        Ok(Bytes::from(window))
    }

    fn warmup_path(&self, _tpe: FileType, _id: &Id) -> String {
        // Cold-storage warm-up never applies: `needs_warm_up` stays false.
        String::new()
    }
}

impl WriteBackend for VolumeBackend {
    fn create(&self) -> RusticResult<()> {
        Err(refuse_write(self))
    }

    fn write_bytes(&self, _tpe: FileType, _id: &Id, _cacheable: bool, _content: BytesList) -> RusticResult<()> {
        Err(refuse_write(self))
    }

    fn remove(&self, _tpe: FileType, _id: &Id, _cacheable: bool) -> RusticResult<()> {
        Err(refuse_write(self))
    }
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
