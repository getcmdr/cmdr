//! `LIST_TIMEOUT` counts only the time a listing spends talking to its volume,
//! never the time the backend parks it by its own choice (issue #835: a
//! throttled phone walk turned 26 healthy folders into `listing timed out`).

use super::*;
use cmdr_fs::volume::ScanListingClock;

/// A volume whose scan listing first parks (clock paused, like MTP at its yield
/// point or behind the device lock) for `parked`, then works (clock running) for
/// `working`, then answers with the inner listing.
struct ParkingVolume {
    inner: InMemoryVolume,
    parked: Duration,
    working: Duration,
}

impl Volume for ParkingVolume {
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
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> ListFut<'a, Vec<FileEntry>> {
        self.inner.list_directory(path, on_progress)
    }
    fn list_directory_for_scan<'a>(
        &'a self,
        path: &'a AppPath,
        _cancel: Option<&'a CancellationToken>,
        clock: &'a ScanListingClock,
    ) -> ListFut<'a, Vec<FileEntry>> {
        Box::pin(async move {
            // allowed-test-sleep: fake latency is the subject, on a paused runtime.
            clock.paused(tokio::time::sleep(self.parked)).await;
            // allowed-test-sleep: fake latency is the subject, on a paused runtime.
            tokio::time::sleep(self.working).await;
            self.inner.list_directory(path, None).await
        })
    }
    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> ListFut<'a, FileEntry> {
        self.inner.get_metadata(path)
    }
    fn exists<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        self.inner.exists(path)
    }
    fn is_directory<'a>(&'a self, path: &'a AppPath) -> ListFut<'a, bool> {
        self.inner.is_directory(path)
    }
}

fn parking(parked: Duration, working: Duration) -> Arc<dyn Volume> {
    Arc::new(ParkingVolume {
        inner: wide_tree(3),
        parked,
        working,
    })
}

#[tokio::test(start_paused = true)]
async fn a_listing_parked_by_its_backend_far_past_the_timeout_still_lists() {
    let volume = parking(LIST_TIMEOUT * 5, Duration::from_secs(1));
    let listed = list_one_directory(volume, cmdr_fs::app_path!("/").to_owned(), CancellationToken::new()).await;
    assert_eq!(
        listed.expect("a parked listing is our own pacing, not a wedge").len(),
        3
    );
}

#[tokio::test(start_paused = true)]
async fn a_round_trip_that_never_answers_still_times_out() {
    let volume = parking(Duration::ZERO, Duration::from_secs(60 * 60));
    let started = tokio::time::Instant::now();
    let listed = list_one_directory(
        volume,
        cmdr_fs::app_path!("/wedged").to_owned(),
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(listed, Err(VolumeScanError::Timeout(_))), "got {listed:?}");
    assert_eq!(started.elapsed(), LIST_TIMEOUT, "times out after exactly the budget");
}

#[tokio::test(start_paused = true)]
async fn only_the_time_after_parking_counts_toward_the_timeout() {
    let parked = Duration::from_secs(600);
    let volume = parking(parked, Duration::from_secs(60 * 60));
    let started = tokio::time::Instant::now();
    let listed = list_one_directory(
        volume,
        cmdr_fs::app_path!("/wedged").to_owned(),
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(listed, Err(VolumeScanError::Timeout(_))), "got {listed:?}");
    assert_eq!(started.elapsed(), parked + LIST_TIMEOUT);
}
