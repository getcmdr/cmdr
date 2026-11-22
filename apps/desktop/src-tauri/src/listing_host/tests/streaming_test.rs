//! The app's half of a streaming read: a real local folder through the local
//! backend's reader, and the foreground hold reaching `priority::foreground`.
//! The engine's own cells, over fakes, are `crates/cmdr-listing/src/streaming_test.rs`.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio_util::sync::CancellationToken;

use crate::file_system::volume::manager::get_volume_manager;
use crate::listing_host::listings;
use crate::priority::foreground;
use crate::test_support::{TestDir, wait_until_async};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::unique_test_id;
use cmdr_fs::volume::{ListingProgress, Volume, VolumeError};
use cmdr_listing::ListingId;
use cmdr_listing::testing::{TestEngine, TestListingGuard, request};
use cmdr_local::LocalVolume;

/// Budget for a read to reach the backend.
const STARTS_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// The whole chain a real local folder takes to the pane's "Loaded N files..." line:
/// `read_directory_with_progress` -> `Volume::list_directory` -> `listing-progress`. The
/// other tests here run on `InMemoryVolume`, which can't catch a local backend that drops
/// its `on_progress`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_local_directory_read_emits_progress_events() {
    let engine = TestEngine::new();
    let dir = TestDir::new("streaming_local_progress");
    for i in 0..5_000 {
        std::fs::write(dir.join(format!("file_{i:05}.txt")), b"x").expect("writing a scratch file succeeds");
    }

    let volume_id = &format!("test-local-progress-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(&engine, unique_test_id("streaming-local-progress"));
    engine.volumes.register(
        volume_id,
        Arc::new(LocalVolume::local_folder(
            "Test Volume",
            &*dir,
            crate::volume_host::host(),
        )),
    );

    let sink = &engine.events;
    let state = engine.streaming_state();

    let result = engine
        .read_with(
            &state,
            &request(
                listing.id(),
                volume_id,
                crate::test_support::native_app_path(&dir).as_str(),
            ),
        )
        .await;
    assert!(result.is_ok(), "listing the scratch dir succeeds: {:?}", result.err());

    let progress = sink.progress.lock_ignore_poison();
    assert!(
        !progress.is_empty(),
        "reading 5,000 local entries emitted no listing-progress, so the pane would sit on \"Opening folder…\""
    );
    assert!(
        progress.iter().all(|(id, _)| id == listing.id()),
        "progress must be tagged with the listing that asked for it"
    );
    assert_eq!(
        sink.read_complete.lock_ignore_poison().first().map(|(_, total)| *total),
        Some(5_000),
        "read-complete reports the full count"
    );
}

/// A listing holds a foreground lease for as long as it actually runs, not for the
/// half-second the entry-point timestamp survives. That is what keeps a background
/// SMB upload and the index scan standing aside for a folder that takes seconds to
/// come back, instead of resuming while the user is still watching a spinner. This
/// is the app's lease (`AppLeases` over `priority::foreground`); the engine's
/// hold-for-the-whole-read is `crates/cmdr-listing/src/streaming_test.rs`'s.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listing_holds_a_foreground_lease_for_its_whole_duration() {
    let volume_id = &format!("test-lease-{}", uuid::Uuid::new_v4());
    let listing = TestListingGuard::adopt(listings(), unique_test_id("streaming-lease"));

    let volume = Arc::new(WaitsForCancel::new());
    let started = Arc::clone(&volume.started);
    get_volume_manager().register(volume_id, Arc::clone(&volume) as Arc<dyn Volume>);

    assert_eq!(
        foreground::global().volume_lease_count(volume_id),
        0,
        "nobody has listed this volume yet"
    );

    let state = listings().streaming_state();
    let read = {
        let state = Arc::clone(&state);
        let volume_id = volume_id.clone();
        let listing_id = listing.id().to_string();
        tokio::spawn(async move {
            listings()
                .read_with(&state, &request(&listing_id, &volume_id, "/"))
                .await
        })
    };

    wait_until_async(STARTS_WITHIN, "the listing to reach the backend", || {
        started.load(Ordering::SeqCst)
    })
    .await;
    assert_eq!(
        foreground::global().volume_lease_count(volume_id),
        1,
        "the share must read busy while the listing is still on the wire"
    );

    state.cancel.cancel();
    read.await
        .expect("listing task must not panic")
        .expect("a cancelled listing still returns Ok");

    assert_eq!(
        foreground::global().volume_lease_count(volume_id),
        0,
        "a cancelled listing gives its lease back: the pane moved on, so nobody is waiting on it"
    );
    assert!(
        !foreground::global().idle_for_volume(volume_id, std::time::Duration::from_secs(30)),
        "the listing stamped the share, so the debounce keeps background work off it afterwards"
    );

    listings().end(&ListingId::new(listing.id()));
    get_volume_manager().unregister(volume_id);
}

/// A volume whose read waits until the listing is cancelled, and says when it
/// started.
struct WaitsForCancel {
    root: &'static AppPath,
    started: Arc<AtomicBool>,
}

impl WaitsForCancel {
    fn new() -> Self {
        Self {
            root: app_path!("/"),
            started: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Volume for WaitsForCancel {
    fn name(&self) -> &str {
        "Waits for cancel"
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
        Box::pin(async { Err(VolumeError::NotSupported) })
    }

    fn list_directory_with_cancel<'a>(
        &'a self,
        _path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
        cancel: Option<&'a CancellationToken>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        let cancel = cancel.cloned().expect("a pane listing hands its read the cancel token");
        let started = Arc::clone(&self.started);
        Box::pin(async move {
            started.store(true, Ordering::SeqCst);
            // Bounded, so a lost cancel fails the test instead of hanging the suite.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(30), cancel.cancelled()).await;
            Ok(Vec::new())
        })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        Box::pin(async move { Err(VolumeError::NotFound(path.as_str().to_string())) })
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
