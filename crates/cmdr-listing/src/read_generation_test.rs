//! No full re-read of a listing lands after a newer one already did.
//!
//! Every full re-read (a `FullRefresh`, ⌘R and the watcher's re-read, a respell,
//! a backend's `Replaced`) takes a read generation when it STARTS reading, and
//! the replace step drops it when a newer generation already published. The
//! per-directory turnstile still bounds a storm to two reads; it just isn't what
//! orders a ⌘R against a `FullRefresh` any more, since the ⌘R never queues on it.

use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::app_path::{AppName, AppPath, PathArithmetic};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::{IgnorePoison, RwLockIgnorePoison};
use cmdr_fs::testing::unique_test_id;
use cmdr_fs::volume::{
    BatchScanResult, CopyScanResult, InMemoryVolume, ScanConflict, SourceItemInfo, SpaceInfo, Volume, VolumeError,
    VolumeReadStream,
};
use tokio::sync::Notify;

use super::Listings;
use super::testing::{TestEngine, TestListing};
use cmdr_fs::testing::wait_until_async;

pub(super) fn entry(dir: &AppPath, name: &str) -> FileEntry {
    FileEntry::new(
        name.to_string(),
        dir.join(AppName::from_app_spelling(name).expect("one plain name"))
            .expect("joins")
            .into_string(),
        false,
        false,
    )
}

/// One scripted `list_directory` answer: the entries, and a gate the read waits
/// on before it answers (`None` answers at once).
pub(super) struct Read {
    pub(super) gate: Option<Arc<Notify>>,
    pub(super) entries: Vec<FileEntry>,
}

/// A volume whose reads answer from a script, each one optionally held open
/// until the test releases it, counting how many reads have started. That
/// makes "the read that started first finishes last" an ordering the test
/// drives, with no timing in it. A single stat answers from `disk`, the
/// directory as it stands at the moment of the stat, whatever the scripted reads
/// said; a stat can be held open on `stat_gate` too, after it answered.
pub(super) struct GatedListVolume {
    inner: InMemoryVolume,
    pub(super) script: Mutex<VecDeque<Read>>,
    started: AtomicUsize,
    disk: Mutex<HashMap<String, FileEntry>>,
    /// How many of the next reads fail, ahead of the script.
    pub(super) failing_reads: AtomicUsize,
    /// Paths whose stat can't tell (a refusal-like error, never `NotFound`).
    pub(super) unanswerable: Mutex<HashSet<String>>,
    /// Held stats wait here once they've looked at the disk.
    pub(super) stat_gate: Mutex<Option<Arc<Notify>>>,
    stats: AtomicUsize,
    /// What `stat_lists_the_parent` answers.
    pub(super) stat_lists_the_parent: AtomicBool,
}

impl GatedListVolume {
    fn new(script: Vec<Read>) -> Self {
        Self {
            inner: InMemoryVolume::new("gated"),
            script: Mutex::new(script.into()),
            started: AtomicUsize::new(0),
            disk: Mutex::new(HashMap::new()),
            failing_reads: AtomicUsize::new(0),
            unanswerable: Mutex::new(HashSet::new()),
            stat_gate: Mutex::new(None),
            stats: AtomicUsize::new(0),
            stat_lists_the_parent: AtomicBool::new(false),
        }
    }

    pub(super) fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    /// How many stats have looked at the disk.
    pub(super) fn stats(&self) -> usize {
        self.stats.load(Ordering::SeqCst)
    }

    /// Puts `entry` on the disk a stat sees, replacing what was there.
    pub(super) fn put(&self, entry: FileEntry) {
        self.disk.lock_ignore_poison().insert(entry.path.clone(), entry);
    }

    /// Takes the entry at `path` off the disk a stat sees.
    pub(super) fn delete(&self, path: &str) {
        self.disk.lock_ignore_poison().remove(path);
    }
}

impl Volume for GatedListVolume {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn root(&self) -> &AppPath {
        self.inner.root()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn list_directory<'a>(
        &'a self,
        _path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(cmdr_fs::volume::ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let fails = self
            .failing_reads
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if fails {
            return Box::pin(async { Err(VolumeError::NotSupported) });
        }
        let next = self.script.lock_ignore_poison().pop_front();
        Box::pin(async move {
            let Some(read) = next else {
                return Ok(Vec::new());
            };
            if let Some(gate) = read.gate {
                gate.notified().await;
            }
            Ok(read.entries)
        })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        let answer = if self.unanswerable.lock_ignore_poison().contains(path.as_str()) {
            Err(VolumeError::NotSupported)
        } else {
            match self.disk.lock_ignore_poison().get(path.as_str()) {
                Some(entry) => Ok(entry.clone()),
                None => Err(VolumeError::NotFound(path.as_str().to_owned())),
            }
        };
        self.stats.fetch_add(1, Ordering::SeqCst);
        let gate = self.stat_gate.lock_ignore_poison().clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            answer
        })
    }

    fn stat_lists_the_parent(&self) -> bool {
        self.stat_lists_the_parent.load(Ordering::SeqCst)
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

    fn get_space_info<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        self.inner.get_space_info()
    }

    fn scan_for_copy<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<CopyScanResult, VolumeError>> + Send + 'a>> {
        self.inner.scan_for_copy(path)
    }

    fn scan_for_copy_batch<'a>(
        &'a self,
        paths: &'a [AppPathBuf],
    ) -> Pin<Box<dyn Future<Output = Result<BatchScanResult, VolumeError>> + Send + 'a>> {
        self.inner.scan_for_copy_batch(paths)
    }

    fn scan_for_conflicts<'a>(
        &'a self,
        source_items: &'a [SourceItemInfo],
        dest_path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ScanConflict>, VolumeError>> + Send + 'a>> {
        self.inner.scan_for_conflicts(source_items, dest_path)
    }

    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        self.inner.open_read_stream(path)
    }
}

/// An engine with a fresh volume id and directory, the volume registered under it.
pub(super) fn register(tag: &str, script: Vec<Read>) -> (TestEngine, String, AppPathBuf, Arc<GatedListVolume>) {
    let volume_id = unique_test_id(&format!("{tag}-vol"));
    let dir = super::testing::app_text(&format!("/test/{}", unique_test_id(&format!("{tag}-dir"))));
    let volume = Arc::new(GatedListVolume::new(script));
    let engine = TestEngine::builder()
        .volume(&volume_id, Arc::clone(&volume) as Arc<dyn Volume>)
        .build();
    (engine, volume_id, dir, volume)
}

/// `engine`'s `FullRefresh` of `dir` for `listing_id`, on a task of its own.
pub(super) fn spawn_full_refresh(
    engine: &Listings,
    volume_id: &str,
    dir: &AppPath,
    listing_id: &str,
) -> tokio::task::JoinHandle<()> {
    let inner = Arc::clone(&engine.inner);
    let (volume_id, dir, listings) = (volume_id.to_string(), dir.to_owned(), sort_of(listing_id));
    tokio::spawn(async move { inner.notify_full_refresh(volume_id, dir, listings).await })
}

pub(super) fn names(entries: Vec<FileEntry>) -> Vec<String> {
    entries.into_iter().map(|e| e.name).collect()
}

fn sort_of(id: &str) -> Vec<String> {
    vec![id.to_string()]
}

/// Whether a `FullRefresh` of `dir` is queued behind the running one.
fn refresh_queued(engine: &Listings, volume_id: &str, dir: &AppPath) -> bool {
    engine
        .inner
        .refresh_slots
        .read_ignore_poison()
        .get(&(volume_id.to_string(), dir.to_owned()))
        .is_some_and(|slot| slot.queued.load(Ordering::Acquire))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_full_refresh_cannot_land_over_a_newer_refresh() {
    // A `FullRefresh` starts reading a folder mid-write and is slow to answer; a ⌘R
    // starts after it, reads the finished folder, and lands first. The ⌘R never
    // queues on the turnstile, so without a generation the slow read lands last and
    // the pane shows the half-written folder until something else changes it.
    let gate = Arc::new(Notify::new());
    // Scripted below, once the directory's name is known.
    let (engine, volume_id, dir, volume) = register("gen-slow-first", Vec::new());
    let mid_write = vec![entry(&dir, "alpha")];
    let complete = vec![entry(&dir, "alpha"), entry(&dir, "bravo"), entry(&dir, "charlie")];
    {
        let mut script = volume.script.lock_ignore_poison();
        script.push_back(Read {
            gate: Some(Arc::clone(&gate)),
            entries: mid_write.clone(),
        });
        script.push_back(Read {
            gate: None,
            entries: complete.clone(),
        });
    }
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(dir.as_str())
        .entries(mid_write)
        .insert(&engine, "gen-slow-first");

    let slow = spawn_full_refresh(&engine, &volume_id, &dir, listing.id());
    wait_until_async(Duration::from_secs(5), "the FullRefresh to start reading", || {
        volume.started() == 1
    })
    .await;

    engine.refresh(&listing.listing_id()).await;
    assert_eq!(names(listing.entries()), ["alpha", "bravo", "charlie"]);

    gate.notify_one();
    slow.await.expect("the FullRefresh task ends");

    assert_eq!(
        names(listing.entries()),
        ["alpha", "bravo", "charlie"],
        "the FullRefresh started reading before the ⌘R did, so its older read must not land over the newer one"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_queued_full_refresh_still_lands_after_a_refresh_that_published_while_it_waited() {
    // The generation is taken when a read STARTS, after its turn, never when it was
    // queued: this `FullRefresh` waits behind a held turn while a ⌘R publishes, and
    // then reads the folder LATER than that ⌘R did, so its read is the newer one.
    let (engine, volume_id, dir, volume) = register("gen-queued", Vec::new());
    let read_by_refresh = vec![entry(&dir, "alpha")];
    let read_by_queued = vec![entry(&dir, "alpha"), entry(&dir, "bravo")];
    {
        let mut script = volume.script.lock_ignore_poison();
        script.push_back(Read {
            gate: None,
            entries: read_by_refresh,
        });
        script.push_back(Read {
            gate: None,
            entries: read_by_queued,
        });
    }
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(dir.as_str())
        .entries(Vec::new())
        .insert(&engine, "gen-queued");

    let turn = engine.inner.refresh_turn(&volume_id, &dir).await;
    let queued = spawn_full_refresh(&engine, &volume_id, &dir, listing.id());
    wait_until_async(
        Duration::from_secs(5),
        "the FullRefresh to queue behind the held turn",
        || refresh_queued(&engine, &volume_id, &dir),
    )
    .await;

    engine.refresh(&listing.listing_id()).await;
    assert_eq!(names(listing.entries()), ["alpha"]);

    drop(turn);
    queued.await.expect("the FullRefresh task ends");

    assert_eq!(volume.started(), 2);
    assert_eq!(
        names(listing.entries()),
        ["alpha", "bravo"],
        "the queued FullRefresh read after the ⌘R published, so it must land"
    );
}
