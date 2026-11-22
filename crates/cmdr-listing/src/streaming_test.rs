//! Tests for the streaming directory listing pipeline.
//!
//! Each test reads through an engine of its own: volumes it registered,
//! reports recorded, foreground holds counted.

use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

use crate::Listings;
use crate::testing::StreamingListingState;
use crate::testing::{TestEngine, TestListingGuard, request};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::testing::unique_test_id;
use cmdr_fs::testing::wait_until_async;
use cmdr_fs::volume::{InMemoryVolume, ListingProgress, Volume, VolumeError};

/// Creates a test file entry under the root directory.
fn test_entry(name: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        size: if is_dir { None } else { Some(1024) },
        modified_at: Some(1_640_000_000),
        created_at: Some(1_639_000_000),
        permissions: if is_dir { 0o755 } else { 0o644 },
        owner: "testuser".to_string(),
        group: "staff".to_string(),
        ..FileEntry::new(name.to_string(), format!("/{}", name), is_dir, false)
    }
}

/// Registers an `InMemoryVolume` holding `entries` in `engine` under `volume_id`.
fn register_test_volume(engine: &TestEngine, volume_id: &str, entries: Vec<FileEntry>) {
    let volume = Arc::new(InMemoryVolume::with_entries("Test Volume", entries));
    engine.volumes.register(volume_id, volume);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_streaming_list_populates_cache() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-cache-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-cache"));

    let entries = vec![
        test_entry("photos", true),
        test_entry("zebra.txt", false),
        test_entry("apple.txt", false),
    ];
    register_test_volume(&engine, volume_id, entries);

    let state = engine.streaming_state();

    let result = engine.read_with(&state, &request(listing.id(), volume_id, "/")).await;

    assert!(result.is_ok(), "Expected Ok, got {:?}", result);

    // Verify cache
    listing.with_listing(|cached| {
        assert_eq!(cached.entries().len(), 3);
        // Dirs first, then alpha
        assert_eq!(cached.entries()[0].name, "photos");
        assert!(cached.entries()[0].is_directory);
        assert_eq!(cached.entries()[1].name, "apple.txt");
        assert_eq!(cached.entries()[2].name, "zebra.txt");
    });

    // Verify complete event
    let complete = engine.events.complete.lock().unwrap();
    assert_eq!(complete.len(), 1);
    assert_eq!(complete[0].1, 3); // total_count
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_streaming_list_emits_opening_and_complete() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-events-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-events"));

    register_test_volume(&engine, volume_id, vec![test_entry("file.txt", false)]);

    let sink = &engine.events;
    let state = engine.streaming_state();

    let result = engine.read_with(&state, &request(listing.id(), volume_id, "/")).await;

    assert!(result.is_ok());

    let opening = sink.opening.lock().unwrap();
    assert_eq!(opening.len(), 1);
    assert_eq!(opening[0], listing.id());

    let complete = sink.complete.lock().unwrap();
    assert_eq!(complete.len(), 1);
    assert_eq!(complete[0].0, listing.id());

    let read_complete = sink.read_complete.lock().unwrap();
    assert_eq!(read_complete.len(), 1);

    // No errors or cancellations
    assert!(sink.failed.lock().unwrap().is_empty());
    assert!(sink.cancelled.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_streaming_list_cancellation() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-cancel-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-cancel"));

    register_test_volume(&engine, volume_id, vec![test_entry("file.txt", false)]);

    let sink = &engine.events;
    let state = engine.streaming_state();

    // Set cancelled BEFORE calling
    state.cancel.cancel();

    let result = engine.read_with(&state, &request(listing.id(), volume_id, "/")).await;

    assert!(result.is_ok());

    // Cancelled event should be emitted
    let cancelled = sink.cancelled.lock().unwrap();
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0], listing.id());

    // No entries cached
    assert!(!listing.is_cached());

    // No complete event
    assert!(sink.complete.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_streaming_list_volume_not_found() {
    let engine = TestEngine::new();
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-notfound"));
    let volume_id = &format!("nonexistent-volume-{}", uuid::Uuid::new_v4());

    let state = engine.streaming_state();

    let result = engine.read_with(&state, &request(listing.id(), volume_id, "/")).await;

    match result {
        Err(VolumeError::NotFound(path)) => assert_eq!(path, "/", "NotFound carries the listed path"),
        other => panic!("Expected VolumeError::NotFound, got {:?}", other),
    }
}

/// An id nothing serves, which the host says is a server nobody connected yet:
/// the read answers `NotConnected`, never the `NotFound` the frontend reads as
/// "this folder was deleted".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_streaming_list_on_a_volume_nobody_connected_is_not_connected() {
    let engine = TestEngine::new();
    engine.volumes.not_connected("saved-server");
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-not-connected"));

    let result = engine
        .read_with(&engine.streaming_state(), &request(listing.id(), "saved-server", "/"))
        .await;

    assert!(matches!(result, Err(VolumeError::NotConnected(_))), "got {result:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_streaming_list_empty_directory() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-empty-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-empty"));

    register_test_volume(&engine, volume_id, vec![]);

    let sink = &engine.events;
    let state = engine.streaming_state();

    let result = engine.read_with(&state, &request(listing.id(), volume_id, "/")).await;

    assert!(result.is_ok());

    // Cache should have 0 entries (`with_listing` panics if it wasn't cached at all)
    listing.with_listing(|cached| assert_eq!(cached.entries().len(), 0));

    // Complete should report 0
    let complete = sink.complete.lock().unwrap();
    assert_eq!(complete.len(), 1);
    assert_eq!(complete[0].1, 0);
}

/// A pane that asks for a directory by a spelling its volume doesn't store (the
/// kernel mount's NFD, carried into a direct SMB connection) lands on the stored
/// one: the cache keys the listing under the server's bytes, which is what the
/// watcher reports changes under, and the frontend is told to adopt it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_foreign_spelling_lands_the_listing_on_the_stored_path() {
    let engine = TestEngine::new();
    use crate::testing::SpelledVolume;

    let stored = "/fot\u{f3}k";
    let foreign = "/foto\u{301}k";
    let volume_id = &format!("test-spelling-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-spelling"));
    let inner = InMemoryVolume::new("Share");
    inner.create_directory(crate::testing::app(stored)).await.unwrap();
    inner
        .create_file(&crate::testing::app_child(stored, "photo.jpg"), b"jpeg")
        .await
        .unwrap();
    let volume = SpelledVolume::new(inner).resolving(foreign, Ok(Some(stored)));
    engine.volumes.register(volume_id, Arc::new(volume));

    let sink = &engine.events;
    let result = engine
        .read_with(&engine.streaming_state(), &request(listing.id(), volume_id, foreign))
        .await;

    assert!(result.is_ok(), "the foreign spelling must list, got {result:?}");
    listing.with_listing(|cached| {
        assert_eq!(
            cached.path.as_path(),
            crate::testing::app(stored),
            "keyed under the stored bytes"
        );
        assert!(cached.entries()[0].path.starts_with(stored));
    });
    let stored_paths = sink.stored_paths.lock().unwrap();
    assert_eq!(
        *stored_paths,
        vec![(listing.id().to_string(), Some(crate::testing::app(stored).to_owned()))],
        "the pane must learn the stored spelling"
    );
}

/// A volume whose listing only ends when its cancel flag flips: the stand-in for
/// an MTP device, where the listing is a long chain of USB round trips and the
/// backend bails between them.
///
/// Records which of the two exits happened. `finished` means the listing future
/// ran to its own cooperative end; `aborted` means it was dropped mid-listing,
/// which on a real phone abandons a PTP transaction and wedges the device.
struct CooperativeCancelVolume {
    root: &'static AppPath,
    started: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
}

/// Flips `aborted` unless disarmed, so a dropped listing future is observable.
struct AbortWitness {
    aborted: Arc<AtomicBool>,
    armed: bool,
}

impl Drop for AbortWitness {
    fn drop(&mut self) {
        if self.armed {
            self.aborted.store(true, Ordering::SeqCst);
        }
    }
}

impl CooperativeCancelVolume {
    fn new() -> Self {
        Self {
            root: cmdr_fs::app_path!("/"),
            started: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            aborted: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The listing body both `list_directory` and `list_directory_with_cancel`
    /// run: spin until the token flips, with a hard iteration cap so a missing
    /// token fails the test instead of hanging the suite.
    fn listing_body(
        &self,
        cancel: Option<CancellationToken>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'static>> {
        let started = Arc::clone(&self.started);
        let finished = Arc::clone(&self.finished);
        let aborted = Arc::clone(&self.aborted);
        Box::pin(async move {
            let mut witness = AbortWitness { aborted, armed: true };
            started.store(true, Ordering::SeqCst);
            for _ in 0..2_000 {
                if cancel.as_ref().is_some_and(CancellationToken::is_cancelled) {
                    break;
                }
                // allowed-test-sleep: this fake backend simulates a long, cancellable listing; the
                // per-iteration wait is what keeps it in flight long enough for a cancel to land mid-run
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            witness.armed = false;
            finished.store(true, Ordering::SeqCst);
            Ok(Vec::new())
        })
    }
}

impl Volume for CooperativeCancelVolume {
    fn name(&self) -> &str {
        "Cooperative cancel volume"
    }

    fn root(&self) -> &AppPath {
        self.root
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn list_directory<'a>(
        &'a self,
        _path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        // No token: this is the shape that can only be stopped by dropping the
        // future, which is exactly what must not happen.
        self.listing_body(None)
    }

    fn list_directory_with_cancel<'a>(
        &'a self,
        _path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
        cancel: Option<&'a CancellationToken>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        self.listing_body(cancel.cloned())
    }

    fn get_metadata<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        Box::pin(async { Err(VolumeError::NotFound("not implemented".to_string())) })
    }

    fn exists<'a>(&'a self, _path: &'a AppPath) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async { true })
    }

    fn is_directory<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        Box::pin(async { Ok(true) })
    }
}

/// The budget for either flag below: the backend flips both within a few polls, so a
/// timeout means the cancel path stopped working, never load.
const FLAG_FLIPS_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// Cancelling a listing must let the backend unwind at its own safe boundary,
/// never drop its future. On MTP a dropped future abandons an in-flight PTP
/// transaction and wedges the phone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cancel_unwinds_the_listing_instead_of_aborting_it() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-coop-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-coop"));

    let volume = Arc::new(CooperativeCancelVolume::new());
    let started = Arc::clone(&volume.started);
    let finished = Arc::clone(&volume.finished);
    let aborted = Arc::clone(&volume.aborted);
    engine
        .volumes
        .register(volume_id, Arc::clone(&volume) as Arc<dyn Volume>);

    let sink = &engine.events;
    let state = engine.streaming_state();

    let read = {
        let engine = Listings::clone(&engine);
        let state = Arc::clone(&state);
        let volume_id = volume_id.clone();
        let listing_id = listing.id().to_string();
        tokio::spawn(async move { engine.read_with(&state, &request(&listing_id, &volume_id, "/")).await })
    };

    wait_until_async(FLAG_FLIPS_WITHIN, "the listing to start before we cancel it", || {
        started.load(Ordering::SeqCst)
    })
    .await;

    // The same one step as `cancel_listing`.
    state.cancel.cancel();

    let result = read.await.expect("listing task must not panic");
    assert!(result.is_ok());
    assert_eq!(
        sink.cancelled.lock().unwrap().len(),
        1,
        "the user must see a prompt cancel"
    );

    wait_until_async(
        FLAG_FLIPS_WITHIN,
        "the backend to reach its own cooperative end after a cancel",
        || finished.load(Ordering::SeqCst),
    )
    .await;
    assert!(
        !aborted.load(Ordering::SeqCst),
        "the listing future was dropped mid-flight; on MTP that abandons a PTP transaction and wedges the device"
    );
}

/// A listing holds a foreground lease for as long as it actually runs, not for the
/// half-second the entry-point timestamp survives. That is what keeps a background
/// SMB upload and the index scan standing aside for a folder that takes seconds to
/// come back, instead of resuming while the user is still watching a spinner.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listing_holds_a_foreground_lease_for_its_whole_duration() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-lease-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-lease"));

    let volume = Arc::new(CooperativeCancelVolume::new());
    let started = Arc::clone(&volume.started);
    engine
        .volumes
        .register(volume_id, Arc::clone(&volume) as Arc<dyn Volume>);

    assert_eq!(engine.leases.held(), 0, "nobody has listed this volume yet");

    let state = engine.streaming_state();
    let read = {
        let engine = Listings::clone(&engine);
        let state = Arc::clone(&state);
        let volume_id = volume_id.clone();
        let listing_id = listing.id().to_string();
        tokio::spawn(async move { engine.read_with(&state, &request(&listing_id, &volume_id, "/")).await })
    };

    wait_until_async(FLAG_FLIPS_WITHIN, "the listing to reach the backend", || {
        started.load(Ordering::SeqCst)
    })
    .await;
    assert_eq!(
        engine.leases.held(),
        1,
        "the share must read busy while the listing is still on the wire"
    );

    state.cancel.cancel();
    read.await
        .expect("listing task must not panic")
        .expect("a cancelled listing still returns Ok");

    assert_eq!(
        engine.leases.held(),
        0,
        "a cancelled listing gives its lease back: the pane moved on, so nobody is waiting on it"
    );
    assert_eq!(
        engine.leases.taken(),
        [volume_id.as_str()],
        "the hold was this volume's (the app's half, the debounce stamp, is `listing_host/tests/`)"
    );
}

/// The error path gives the lease back too. A volume that isn't there returns
/// before any of the happy-path exits run, so a lease released only on success
/// would pin the share busy for the rest of the session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listing_that_fails_gives_its_lease_back() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-lease-missing-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-lease-error"));

    let state = engine.streaming_state();
    let result = engine.read_with(&state, &request(listing.id(), volume_id, "/")).await;

    assert!(result.is_err(), "an unregistered volume can't be listed");
    assert_eq!(engine.leases.held(), 0);
}

/// Two panes listing one share: the volume stays busy until the LAST listing ends.
/// A flag instead of a count would free the share the moment either one finished.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_listings_on_one_volume_both_have_to_finish() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-lease-pair-{}", uuid::Uuid::new_v4());
    let left = TestListingGuard::adopt(&engine, unique_test_id("streaming-lease-left"));
    let right = TestListingGuard::adopt(&engine, unique_test_id("streaming-lease-right"));

    let volume = Arc::new(CooperativeCancelVolume::new());
    engine
        .volumes
        .register(volume_id, Arc::clone(&volume) as Arc<dyn Volume>);

    let spawn_listing = |listing_id: String, state: Arc<StreamingListingState>| {
        let volume_id = volume_id.clone();
        let engine = Listings::clone(&engine);
        tokio::spawn(async move { engine.read_with(&state, &request(&listing_id, &volume_id, "/")).await })
    };

    let left_state = engine.streaming_state();
    let right_state = engine.streaming_state();
    let left_read = spawn_listing(left.id().to_string(), Arc::clone(&left_state));
    let right_read = spawn_listing(right.id().to_string(), Arc::clone(&right_state));

    wait_until_async(FLAG_FLIPS_WITHIN, "both listings to take their leases", || {
        engine.leases.held() == 2
    })
    .await;

    left_state.cancel.cancel();
    left_read.await.expect("the first listing task must not panic").ok();
    assert_eq!(
        engine.leases.held(),
        1,
        "the second pane is still waiting on this share"
    );

    right_state.cancel.cancel();
    right_read.await.expect("the second listing task must not panic").ok();
    assert_eq!(engine.leases.held(), 0);
}

/// The listing task being DROPPED (app shutdown tears the runtime down mid-listing)
/// must return the lease too. Nothing runs on that path except destructors, which
/// is the whole reason the lease is RAII rather than a pair of calls.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_listing_task_mid_flight_gives_the_lease_back() {
    let engine = TestEngine::new();
    let volume_id = &format!("test-lease-dropped-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-lease-dropped"));

    let volume = Arc::new(CooperativeCancelVolume::new());
    let started = Arc::clone(&volume.started);
    engine
        .volumes
        .register(volume_id, Arc::clone(&volume) as Arc<dyn Volume>);

    let state = engine.streaming_state();
    let read = {
        let engine = Listings::clone(&engine);
        let volume_id = volume_id.clone();
        let listing_id = listing.id().to_string();
        tokio::spawn(async move { engine.read_with(&state, &request(&listing_id, &volume_id, "/")).await })
    };

    wait_until_async(FLAG_FLIPS_WITHIN, "the listing to reach the backend", || {
        started.load(Ordering::SeqCst)
    })
    .await;
    assert_eq!(engine.leases.held(), 1);

    // The one thing that drops a running task's future where it stands.
    read.abort();
    wait_until_async(FLAG_FLIPS_WITHIN, "the dropped task's lease to come back", || {
        engine.leases.held() == 0
    })
    .await;
}

/// A read the volume held past a second is what a busy NAS looks like from here,
/// so its `listing_done` line stands out at `warn`; an ordinary one stays `info`.
#[test]
fn a_listing_whose_read_took_over_a_second_logs_at_warn() {
    use crate::streaming::listing_done_level;
    use std::time::Duration;

    assert_eq!(listing_done_level(Duration::from_millis(40)), log::Level::Info);
    assert_eq!(listing_done_level(Duration::from_millis(999)), log::Level::Info);
    assert_eq!(listing_done_level(Duration::from_millis(1_000)), log::Level::Warn);
    assert_eq!(listing_done_level(Duration::from_millis(6_000)), log::Level::Warn);
}
