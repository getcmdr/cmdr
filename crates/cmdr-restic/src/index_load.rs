//! Loading a repo's full index: one load per repo however many listings wait on
//! it, cancelled when the last of them leaves, with progress in real bytes.
//!
//! rustic's own load reads every index file in parallel on rayon's GLOBAL pool,
//! each read a pool worker blocked in our adapter for a whole NAS round trip
//! (Cmdr's search and icons share that pool), it can't be cancelled, and its
//! progress counts files. So the load reads the index files itself, on the async
//! runtime, counting bytes and checking the cancel between chunks, and streams
//! them to rustic through an [`IndexFeed`]: a bounded window, each file's bytes
//! dropped the moment rustic takes them, so the memory on top of rustic's own
//! stays the same at any index size. rustic decodes them on a small pool of the
//! load's own (`repo::on_own_pool`), never the global one.

use crate::backend::VolumeBackend;
use crate::index_feed::{IndexFeed, Keep, Start};
use crate::repo::{IndexedRepo, ResticError, on_own_pool, open_blocking, worker_crashed};
use bytes::Bytes;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::{ListingPhase, ListingProgress, Volume, VolumeError};
use rustic_core::{FileType, Id};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

/// The most index bytes a load reads ahead of rustic: a handful of restic's
/// index files (~2.5 MB each on David's NAS repo), enough to keep four reads
/// busy while rustic decodes, and the same at any index size.
pub(crate) const WINDOW: u64 = 16 * 1024 * 1024;

/// How many index files rustic decodes at once on the load's pool, each one
/// waiting on the feed for at most one file: one of the pool's threads parks
/// on the channel the others feed.
#[cfg(test)]
const INDEX_DECODERS: usize = crate::repo::RUSTIC_POOL_THREADS - 1;

/// The longest a waiter goes between two progress ticks it forwards, so a pane
/// hears a few a second rather than one per chunk read.
const TICK_EVERY: Duration = Duration::from_millis(100);

/// How a load ended, once it has.
type Outcome = Option<Result<Arc<IndexedRepo>, ResticError>>;

/// What one load reads, and with what.
pub(crate) struct LoadSpec {
    pub(crate) parent: Arc<dyn Volume>,
    /// The repo's folder on `parent`.
    pub(crate) root: PathBuf,
    pub(crate) password: Zeroizing<String>,
    pub(crate) runtime: Handle,
    /// The most index bytes to read ahead of rustic ([`WINDOW`]).
    pub(crate) window: u64,
}

/// One index load in flight, shared by every listing waiting on it.
pub(crate) struct Load {
    cancel: CancellationToken,
    /// How many [`Waiter`]s hold it. Joining and leaving both go through this
    /// lock, so a waiter can't join a load the last one just cancelled.
    waiters: Mutex<usize>,
    /// How far it got. Its own channel, ❗ apart from `outcome`: the loaded
    /// repo's adapter keeps the tally (and so this sender) for good, and an
    /// outcome in the same channel would make the repo hold itself.
    progress: watch::Sender<Option<ListingPhase>>,
    outcome: watch::Sender<Outcome>,
    parent: Arc<dyn Volume>,
}

impl Load {
    /// Starts loading on `spec.runtime`. `finished` runs with the outcome before
    /// any waiter hears it, so a waiter that wakes finds the result installed.
    pub(crate) fn start<F>(spec: LoadSpec, finished: F) -> Arc<Self>
    where
        F: FnOnce(&Result<Arc<IndexedRepo>, ResticError>) + Send + 'static,
    {
        let load = Arc::new(Self {
            cancel: CancellationToken::new(),
            waiters: Mutex::new(0),
            progress: watch::channel(None).0,
            outcome: watch::channel(None).0,
            parent: Arc::clone(&spec.parent),
        });
        let task_load = Arc::clone(&load);
        let runtime = spec.runtime.clone();
        // An outer task around the load, so a load that panics still answers its
        // waiters (a typed error) instead of leaving them waiting forever.
        drop(runtime.spawn(async move {
            let cancel = task_load.cancel.clone();
            let inner_runtime = spec.runtime.clone();
            let progress = task_load.progress.clone();
            let outcome = inner_runtime
                .spawn(run(spec, cancel, progress))
                .await
                .map_err(worker_crashed)
                .and_then(|outcome| outcome)
                .map(Arc::new);
            finished(&outcome);
            task_load.outcome.send_replace(Some(outcome));
        }));
        load
    }

    /// Cancels it whoever still waits: the person forgot the repo's password,
    /// so nobody may have its index. Waiters hear `Cancelled`.
    pub(crate) fn cancel_now(&self) {
        self.cancel.cancel();
    }

    /// Whether this load reads through `parent` (a replaced parent starts anew).
    pub(crate) fn is_over(&self, parent: &Arc<dyn Volume>) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.parent), Arc::as_ptr(parent))
    }

    /// A place among the load's waiters, or `None` once it was cancelled (every
    /// waiter left before it finished), when the caller starts a fresh one.
    pub(crate) fn join(self: &Arc<Self>) -> Option<Waiter> {
        let mut waiters = lock(&self.waiters);
        if self.cancel.is_cancelled() {
            return None;
        }
        *waiters += 1;
        Some(Waiter {
            load: Arc::clone(self),
            last_tick: Mutex::new(None),
        })
    }

    /// A waiter left. The last one to leave a load that hasn't finished cancels
    /// it: nobody wants the index any more, and every read it would still make
    /// is a NAS round trip for nothing.
    fn leave(&self) {
        let mut waiters = lock(&self.waiters);
        *waiters = waiters.saturating_sub(1);
        if *waiters == 0 && self.outcome.borrow().is_none() {
            self.cancel.cancel();
        }
    }

    /// How many waiters hold it.
    #[cfg(test)]
    pub(crate) fn waiters(&self) -> usize {
        *lock(&self.waiters)
    }
}

/// One listing (or stat, or read) waiting on a [`Load`]. Dropping it is
/// leaving: a pane's cancel drops the listing's future, and with it this.
pub(crate) struct Waiter {
    load: Arc<Load>,
    last_tick: Mutex<Option<Instant>>,
}

impl Drop for Waiter {
    fn drop(&mut self) {
        self.load.leave();
    }
}

impl Waiter {
    /// Waits for the load's outcome, handing `on_progress` its phase as the
    /// bytes arrive (a few times a second at most).
    pub(crate) async fn outcome(
        &self,
        on_progress: Option<&(dyn Fn(ListingProgress) + Sync)>,
    ) -> Result<Arc<IndexedRepo>, ResticError> {
        let mut outcome = self.load.outcome.subscribe();
        let mut progress = self.load.progress.subscribe();
        loop {
            let ended = outcome.borrow_and_update().clone();
            if let Some(ended) = ended {
                return ended;
            }
            let phase = *progress.borrow_and_update();
            if let (Some(phase), Some(report)) = (phase, on_progress)
                && self.tick_due()
            {
                report(ListingProgress::in_phase(phase));
            }
            // Both senders live in the load this waiter holds, so neither
            // channel closes while it waits.
            tokio::select! {
                _ = outcome.changed() => {}
                _ = progress.changed() => {}
            }
        }
    }

    fn tick_due(&self) -> bool {
        let mut last = lock(&self.last_tick);
        let due = last.is_none_or(|at| at.elapsed() >= TICK_EVERY);
        if due {
            *last = Some(Instant::now());
        }
        due
    }
}

/// The load itself: list `index/`, open the repo while the first files stream
/// in, then let rustic build the index from the feed on the load's own pool.
async fn run(
    spec: LoadSpec,
    cancel: CancellationToken,
    progress: watch::Sender<Option<ListingPhase>>,
) -> Result<IndexedRepo, ResticError> {
    let index_dir = spec.root.join(FileType::Index.dirname());
    // On its own task, so a cancel DETACHES the listing (dropping the handle),
    // ❌ never drops it mid-transaction: that wedges a phone (MTP).
    let listing = {
        let (parent, dir) = (Arc::clone(&spec.parent), index_dir.clone());
        spec.runtime
            .spawn(async move { index_files(parent.as_ref(), &dir).await })
    };
    let files = tokio::select! {
        biased;
        () = cancel.cancelled() => return Err(cancelled(&index_dir)),
        files = listing => files.map_err(worker_crashed)?.map_err(ResticError::Volume)?,
    };
    let feed = Arc::new(IndexFeed::new(files, spec.window, cancel.clone(), progress));
    let backend = Arc::new(
        VolumeBackend::new(Arc::clone(&spec.parent), spec.root.clone(), spec.runtime.clone())
            .with_index_feed(Arc::clone(&feed)),
    );
    // The reads stream into the feed on their own task the whole load long,
    // the first ones beside the unlock's scrypt (about half a second).
    drop(spec.runtime.spawn(stream_files(
        Arc::clone(&spec.parent),
        index_dir.clone(),
        Arc::clone(&feed),
        spec.runtime.clone(),
    )));
    let build = async {
        let opening = {
            let backend = Arc::clone(&backend);
            let password = spec.password;
            spec.runtime
                .spawn_blocking(move || open_blocking(&backend, &password, None))
        };
        let open = opening.await.map_err(worker_crashed)??;
        let backend = Arc::clone(&backend);
        spec.runtime
            .spawn_blocking(move || on_own_pool(|| IndexedRepo::index(open, backend)))
            .await
            .map_err(worker_crashed)?
    };
    // A cancel answers at once. The blocking open or build is detached: a
    // decoder waiting on the feed hears `Cancelled` and rustic winds down.
    let built = tokio::select! {
        biased;
        () = cancel.cancelled() => Err(cancelled(&index_dir)),
        built = build => built,
    };
    // Built, failed, or cancelled: nothing more is read, and anything rustic
    // didn't take goes.
    match &built {
        Ok(_) => feed.step_aside(),
        Err(_) => feed.close(),
    }
    built
}

/// The index files in `dir`, by id, with their sizes. A missing folder is an
/// empty index.
async fn index_files(parent: &dyn Volume, dir: &Path) -> Result<Vec<(Id, u64)>, VolumeError> {
    let entries = match parent.list_directory(&AppPath::legacy_unchecked(dir), None).await {
        Ok(entries) => entries,
        Err(VolumeError::NotFound(_)) => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    Ok(entries
        .into_iter()
        .filter(|e| !e.is_directory)
        .filter_map(|e| Some((e.name.parse::<Id>().ok()?, e.size.unwrap_or(0))))
        .collect())
}

/// Reads the index files into `feed` as it lets them start, until every one
/// reached rustic's side or the load ended.
///
/// A cancel stops it at once, and each read in flight stops at its next chunk.
/// ❌ Never aborted mid-read: a read dropped inside a device transaction (a repo
/// on a phone) can wedge the device, so the reads are detached, not aborted. A
/// failed read fails the load: the decoder waiting hears it.
async fn stream_files(parent: Arc<dyn Volume>, dir: PathBuf, feed: Arc<IndexFeed>, runtime: Handle) {
    let mut running = JoinSet::new();
    loop {
        while let Some(start) = feed.next_read() {
            let read = read_one(
                Arc::clone(&parent),
                dir.join(&*start.id.to_hex()),
                start,
                Arc::clone(&feed),
            );
            running.spawn_on(read, &runtime);
        }
        if feed.is_done() {
            running.detach_all();
            return;
        }
        let failed = tokio::select! {
            biased;
            () = feed.cancel().cancelled() => {
                running.detach_all();
                feed.close();
                return;
            }
            Some(next) = running.join_next() => match next {
                Ok(Ok(())) => continue,
                Ok(Err(err)) => err,
                Err(crashed) => VolumeError::IoError {
                    message: format!("an index read stopped: {crashed}").into(),
                    raw_os_error: None,
                },
            },
            () = feed.startable() => continue,
        };
        running.detach_all();
        feed.fail(failed);
        return;
    }
}

/// Reads one index file whole into `feed`, counting each chunk as it lands.
/// Stops early when the feed says so (the load ended, or a read ahead ran past
/// what the window gave it).
async fn read_one(
    parent: Arc<dyn Volume>,
    path: PathBuf,
    start: Start,
    feed: Arc<IndexFeed>,
) -> Result<(), VolumeError> {
    // Its task can first run well after the load was cancelled (a busy
    // runtime): a read that never left opens nothing.
    if feed.cancel().is_cancelled() {
        return Ok(());
    }
    let mut stream = parent.open_read_stream(&AppPath::legacy_unchecked(&path)).await?;
    let mut bytes = Vec::with_capacity(usize::try_from(start.reserve).unwrap_or(0));
    while let Some(chunk) = stream.next_chunk().await {
        let chunk = chunk?;
        if feed.add_chunk(start.index, chunk.len()) == Keep::Stop {
            return Ok(());
        }
        bytes.extend_from_slice(&chunk);
    }
    feed.deliver(start.index, Bytes::from(bytes));
    Ok(())
}

#[cfg(test)]
#[path = "index_load_tests.rs"]
mod tests;

fn cancelled(path: &Path) -> ResticError {
    ResticError::Volume(VolumeError::Cancelled(path.display().to_string().into()))
}

/// Locks a std mutex, taking the data over from a thread that panicked holding
/// it: every value here stays whole between statements.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
