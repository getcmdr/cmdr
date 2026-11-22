//! An engine of a test's own, and the fakes it answers its seams with.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, AppPathBuf};

/// Canonical app text as an owned app path, for a test helper's `&str` input.
/// Panics on non-canonical text.
pub fn app_text(text: &str) -> AppPathBuf {
    AppPath::from_normalized(text)
        .unwrap_or_else(|why| panic!("{text:?} is no canonical app path: {why}"))
        .to_owned()
}
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use cmdr_fs::ignore_poison::{IgnorePoison, RwLockIgnorePoison};
use cmdr_fs::volume::{ResolvedVolume, Volume, VolumeError};

use super::events::RecordingEvents;
use crate::host::{
    ForegroundHold, ForegroundLeases, ListingEvents, ListingObserver, ListingOverlay, ListingVolumes, MountFacts,
    MountTable, ReadingLocale, Unregistered,
};
use crate::streaming::StreamingListingState;
use crate::{ListingRequest, Listings};

/// A [`Listings`] a test builds for itself, with every seam it set answered
/// by a fake the test can read back. Derefs to the engine.
///
/// Two of these share nothing: no cache, no diff queue, no observers. So a
/// test needs no unique ids and no teardown, and asserts on engine-wide state
/// (every listing on a path, an orphan sweep) without seeing anyone else's.
pub struct TestEngine {
    listings: Listings,
    /// Every report the engine made (unless the test sent them elsewhere).
    pub events: Arc<RecordingEvents>,
    /// The volume registry the engine lists through.
    pub volumes: Arc<FakeVolumes>,
    /// Every foreground hold the engine took.
    pub leases: Arc<RecordingLeases>,
}

impl TestEngine {
    /// An engine with no volume registered, its reports recorded, names
    /// collating in `en`, and the shared fallback runtime.
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// A [`TestEngine`] with some seams set.
    pub fn builder() -> TestEngineBuilder {
        TestEngineBuilder::default()
    }
}

/// Test-only ways into an engine's streaming read, for a test that holds the
/// read's cancel token itself.
///
/// Each method repeats the module's `testing` gate: that's what tells
/// `index-crate-isolation` these are fixtures, not part of the handle's surface.
impl Listings {
    /// A fresh read's cancel token and stall watch, on this engine.
    #[cfg(any(test, feature = "testing"))]
    pub fn streaming_state(&self) -> Arc<StreamingListingState> {
        Arc::new(StreamingListingState::with_stall_watch(self.inner.stall_watch()))
    }

    /// Runs `request`'s streaming read to its end on the caller's task,
    /// answering to `state`, and returns what the read returned (a refusal the
    /// read's task would report). Progress and the end go to the engine's events.
    #[cfg(any(test, feature = "testing"))]
    pub async fn read_with(
        &self,
        state: &Arc<StreamingListingState>,
        request: &ListingRequest,
    ) -> Result<(), VolumeError> {
        self.inner.read_directory_with_progress(state, request).await
    }

    /// What a [`start`](Self::start) read's task does, on the caller's task:
    /// the read, answering to `state`, then a refusal reported to the engine's
    /// events.
    #[cfg(any(test, feature = "testing"))]
    pub async fn read_as_task(&self, state: &Arc<StreamingListingState>, request: &ListingRequest) {
        let result = self.read_with(state, request).await;
        crate::streaming::report_failure(
            self.inner.events.as_ref(),
            request.id.as_str(),
            &request.volume_id,
            &request.path,
            result,
        );
    }
}

impl Default for TestEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for TestEngine {
    type Target = Listings;

    fn deref(&self) -> &Listings {
        &self.listings
    }
}

/// Runs `request`'s streaming read on `engine` to its end, on the caller's
/// task, and returns what the read returned. Progress, the end, and the
/// observers' open go where a [`Listings::start`] read's do; a refusal comes
/// back here, where a `start` read's task would report it.
pub async fn read_to_end(engine: &Listings, request: &ListingRequest) -> Result<(), VolumeError> {
    engine.read_with(&engine.streaming_state(), request).await
}

/// A request for `path` on `volume_id` under `id`, hidden files shown, sorted
/// Name / Ascending / LikeFiles. Set a field to change one.
///
/// `path` is canonical app text (`/`, `/dir`); anything else panics, so a
/// cell can't silently list a different path.
pub fn request(id: &str, volume_id: &str, path: &str) -> ListingRequest {
    use crate::{DirectorySortMode, ListingId, SortColumn, SortOrder};
    ListingRequest {
        id: ListingId::new(id),
        volume_id: volume_id.to_string(),
        path: app_text(path),
        include_hidden: true,
        sort_by: SortColumn::Name,
        sort_order: SortOrder::Ascending,
        dir_sort_mode: DirectorySortMode::LikeFiles,
    }
}

/// Holds `listing_id`'s diffs on `engine` until the test reads them, so a test
/// sees what a mutation published before the 50 ms flush sends it.
pub fn hold_diffs(engine: &Listings, listing_id: &str) {
    engine.inner.diffs.hold(listing_id);
}

/// Every change `listing_id` has waiting on `engine`, oldest first.
pub fn pending_changes(engine: &Listings, listing_id: &str) -> Vec<crate::DiffChange> {
    engine
        .inner
        .diffs
        .pending_batches(listing_id)
        .into_iter()
        .flat_map(|batch| batch.changes)
        .collect()
}

/// The collator an engine reading `locale` sorts with, asked through its
/// `ReadingLocale` seam the way every sort asks.
pub fn collator_for(locale: &str) -> Arc<crate::NameCollator> {
    TestEngine::builder().locale(locale).build().collator()
}

/// Replaces `listing_id`'s rows on `engine` with `prepared`, as a full re-read
/// that started just now lands, and says how it landed.
#[cfg(test)]
pub(crate) fn replace_entries(
    engine: &Listings,
    listing_id: &str,
    prepared: crate::pipeline::Prepared,
) -> crate::RefreshOutcome {
    let generation = engine.inner.next_read_generation();
    engine.inner.update_listing_entries(listing_id, prepared, generation)
}

/// Hands `entries` to `listing_id` on `engine` the way a backend's `Replaced`
/// change does, with `overlay_rows` of them an overlay's, as a re-read that
/// started just now.
pub fn publish_replacement(
    engine: &Listings,
    listing_id: &str,
    entries: Vec<cmdr_fs::entry::FileEntry>,
    overlay_rows: usize,
) {
    use crate::pipeline::{OverlayRows, Prepared};
    let generation = engine.inner.next_read_generation();
    engine.inner.publish_replacement(
        listing_id,
        Prepared::for_test(entries, OverlayRows::Recounted(overlay_rows)),
        generation,
    );
}

/// One `FullRefresh` of `path` on `volume_id`, as a watcher event drives it,
/// landing on `listing_id` (sorted the listing's own way), awaited to its end:
/// what a refresh does to a listing, without waiting on a spawn.
pub async fn full_refresh(engine: &Listings, volume_id: &str, path: &AppPath, listing_id: &str) {
    engine
        .inner
        .notify_full_refresh(volume_id.to_string(), path.to_owned(), vec![listing_id.to_string()])
        .await;
}

/// Runs `engine`'s overlay step alone on `entries`, a listing of `path` on
/// `volume` (registered as `volume_id`), and returns how many rows it added: what a test pinning one
/// overlay's rows (the shadowing rule, the git portal's six) asserts on.
pub async fn decorate(
    engine: &Listings,
    volume_id: &str,
    volume: &Arc<dyn Volume>,
    path: &AppPath,
    entries: &mut Vec<cmdr_fs::entry::FileEntry>,
) -> usize {
    engine.inner.decorate(volume_id, volume, path, entries).await
}

/// Builds a [`TestEngine`]. Every seam it isn't told about answers with a
/// fake (volumes, events, leases, locale) or its no-op.
pub struct TestEngineBuilder {
    volumes: Arc<FakeVolumes>,
    events: Option<Arc<dyn ListingEvents>>,
    locale: String,
    mounts: Option<Arc<dyn MountTable>>,
    overlays: Vec<Arc<dyn ListingOverlay>>,
    observers: Vec<Arc<dyn ListingObserver>>,
}

impl Default for TestEngineBuilder {
    fn default() -> Self {
        Self {
            volumes: Arc::new(FakeVolumes::default()),
            events: None,
            locale: "en".to_string(),
            mounts: None,
            overlays: Vec::new(),
            observers: Vec::new(),
        }
    }
}

impl TestEngineBuilder {
    /// Registers `volume` under `volume_id` before the engine starts.
    pub fn volume(self, volume_id: &str, volume: Arc<dyn Volume>) -> Self {
        self.volumes.register(volume_id, volume);
        self
    }

    /// Sends the engine's reports to `events` instead of the recorder.
    pub fn events(mut self, events: Arc<dyn ListingEvents>) -> Self {
        self.events = Some(events);
        self
    }

    /// Collates names in `locale` (`en` by default).
    pub fn locale(mut self, locale: &str) -> Self {
        self.locale = locale.to_string();
        self
    }

    /// Answers the mount table from `mounts` (no mounts by default).
    pub fn mounts(mut self, mounts: Arc<dyn MountTable>) -> Self {
        self.mounts = Some(mounts);
        self
    }

    /// Adds `overlay`, after any added before it.
    pub fn overlay(mut self, overlay: Arc<dyn ListingOverlay>) -> Self {
        self.overlays.push(overlay);
        self
    }

    /// Adds `observer`, after any added before it.
    pub fn observer(mut self, observer: Arc<dyn ListingObserver>) -> Self {
        self.observers.push(observer);
        self
    }

    /// Starts the engine.
    pub fn build(self) -> TestEngine {
        let recorder = Arc::new(RecordingEvents::new());
        let leases = Arc::new(RecordingLeases::default());
        let events = self
            .events
            .unwrap_or_else(|| Arc::clone(&recorder) as Arc<dyn ListingEvents>);
        let mut builder = Listings::builder(Arc::clone(&self.volumes) as Arc<dyn ListingVolumes>, events)
            .leases(Arc::clone(&leases) as Arc<dyn ForegroundLeases>)
            .locale(Arc::new(FixedLocale(self.locale)));
        if let Some(mounts) = self.mounts {
            builder = builder.mounts(mounts);
        }
        for overlay in self.overlays {
            builder = builder.overlay(overlay);
        }
        for observer in self.observers {
            builder = builder.observer(observer);
        }
        TestEngine {
            listings: builder.build(),
            events: recorder,
            volumes: self.volumes,
            leases,
        }
    }
}

/// A volume registry a test fills: every id it registered serves, unrouted;
/// any other id answers [`Unregistered::Gone`], or `NotConnected` when the test
/// said so.
// DEFAULT-OK: an empty registry serves nothing, which is what a fresh one does.
#[derive(Default)]
pub struct FakeVolumes {
    volumes: RwLock<HashMap<String, Arc<dyn Volume>>>,
    /// Ids nobody registered that answer `NotConnected`: a saved server or a
    /// listed phone nobody connected yet.
    not_connected: Mutex<HashSet<String>>,
}

impl FakeVolumes {
    /// Serves `volume` under `volume_id`.
    pub fn register(&self, volume_id: &str, volume: Arc<dyn Volume>) {
        self.volumes.write_ignore_poison().insert(volume_id.to_string(), volume);
    }

    /// Makes `volume_id`, while nobody registered it, answer `NotConnected`.
    pub fn not_connected(&self, volume_id: &str) {
        self.not_connected.lock_ignore_poison().insert(volume_id.to_string());
    }
}

impl ListingVolumes for FakeVolumes {
    fn get(&self, volume_id: &str) -> Option<Arc<dyn Volume>> {
        self.volumes.read_ignore_poison().get(volume_id).cloned()
    }

    fn resolve<'a>(
        &'a self,
        volume_id: &'a str,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = ResolvedVolume> + Send + 'a>> {
        Box::pin(async move { self.resolve_local_only(volume_id, path) })
    }

    fn resolve_local_only(&self, volume_id: &str, path: &AppPath) -> ResolvedVolume {
        ResolvedVolume::passthrough_at(self.get(volume_id), path)
    }

    fn unregistered<'a>(
        &'a self,
        volume_id: &'a str,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Unregistered> + Send + 'a>> {
        let not_connected = self.not_connected.lock_ignore_poison().contains(volume_id);
        Box::pin(async move {
            if not_connected {
                Unregistered::NotConnected
            } else {
                Unregistered::Gone
            }
        })
    }
}

/// Collates names in one fixed language.
pub struct FixedLocale(pub String);

impl ReadingLocale for FixedLocale {
    fn locale(&self) -> String {
        self.0.clone()
    }
}

/// Records every foreground hold the engine takes, and how many it still holds.
// DEFAULT-OK: a recorder no read has asked yet holds nothing.
#[derive(Default)]
pub struct RecordingLeases {
    /// The volume id of every hold taken, in order.
    pub taken: Mutex<Vec<String>>,
    held: Arc<AtomicUsize>,
}

impl RecordingLeases {
    /// How many holds are taken and not yet released.
    pub fn held(&self) -> usize {
        self.held.load(Ordering::SeqCst)
    }

    /// The volume ids of every hold taken so far, in order.
    pub fn taken(&self) -> Vec<String> {
        self.taken.lock_ignore_poison().clone()
    }
}

/// Counts down its lease's live holds when the engine drops it.
struct CountedHold(Arc<AtomicUsize>);

impl Drop for CountedHold {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl ForegroundLeases for RecordingLeases {
    fn hold(&self, volume_id: &str) -> ForegroundHold {
        self.taken.lock_ignore_poison().push(volume_id.to_string());
        self.held.fetch_add(1, Ordering::SeqCst);
        ForegroundHold::new(CountedHold(Arc::clone(&self.held)))
    }
}

/// A mount table a test writes: the longest mount point that holds a path
/// answers for it.
// DEFAULT-OK: an empty table answers nothing, the engine's "unknown" mount.
#[derive(Default)]
pub struct FakeMountTable {
    mounts: Vec<(PathBuf, String, String)>,
}

impl FakeMountTable {
    /// Mounts a `fs_type` filesystem from `source` at `mount_point`.
    pub fn mount(mut self, mount_point: &str, fs_type: &str, source: &str) -> Self {
        self.mounts
            .push((PathBuf::from(mount_point), fs_type.to_string(), source.to_string()));
        self
    }
}

impl MountTable for FakeMountTable {
    fn mount_of(&self, path: &Path) -> Option<MountFacts> {
        self.mounts
            .iter()
            .filter(|(point, _, _)| path.starts_with(point))
            .max_by_key(|(point, _, _)| point.components().count())
            .map(|(_, fs_type, source)| MountFacts {
                fs_type: fs_type.clone(),
                source: source.clone(),
            })
    }
}
