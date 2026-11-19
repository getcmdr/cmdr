//! A volume whose connection says whatever the test tells it to: a liveness
//! verdict, and a running count of bytes received.
//!
//! The stall watchdog's one aggressive action is gated on
//! `Volume::connection_liveness` answering `Dead`, and its live receive rate on
//! `Volume::connection_bytes_received`. The one real backend that answers both
//! (`SmbVolume`, off `smb2`'s own readings) needs a server to produce either, so
//! the watchdog's suites script the answers here instead. A test can flip the
//! verdict mid-run, which is how the AND between the verdict and the stillness
//! window gets pinned from both sides. Its twin is the trait default (`None`),
//! which `transfer_probe_tests::a_connection_with_no_liveness_verdict_is_never_aborted`
//! uses to prove a volume with no verdict is only ever reported on.
//!
//! Lives at the `transfer/` level because two sibling suites need it:
//! `transfer_probe_tests.rs` (the watchdog in isolation) and
//! `volume/strategy_retry_tests.rs` (the wedge-to-retry handoff end to end).

use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{ConnectionLiveness, InMemoryVolume, ListingProgress, Volume, VolumeError};

/// No byte count at all, as opposed to a count of zero.
const NO_COUNT: u64 = u64::MAX;

pub(crate) struct ScriptedConnectionVolume {
    inner: Arc<InMemoryVolume>,
    /// 0 = no verdict, 1 = alive, 2 = dead.
    liveness: AtomicU8,
    bytes_received: AtomicU64,
}

impl ScriptedConnectionVolume {
    /// A connection with no verdict and no byte count: what every backend but SMB
    /// answers.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(InMemoryVolume::new("scripted-connection")),
            liveness: AtomicU8::new(0),
            bytes_received: AtomicU64::new(NO_COUNT),
        })
    }

    pub(crate) fn set_liveness(&self, verdict: Option<ConnectionLiveness>) {
        let v = match verdict {
            None => 0,
            Some(ConnectionLiveness::Alive) => 1,
            Some(ConnectionLiveness::Dead) => 2,
        };
        self.liveness.store(v, Ordering::Relaxed);
    }

    /// `n` more bytes arrived on the connection. Starts the count at zero if
    /// there wasn't one.
    pub(crate) fn receive(&self, n: u64) {
        let _ = self
            .bytes_received
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |b| {
                Some(if b == NO_COUNT { n } else { b + n })
            });
    }
}

/// A volume whose connection is proven dead, for the tests where only that
/// matters.
pub(crate) fn dead_connection_volume() -> Arc<dyn Volume> {
    let volume = ScriptedConnectionVolume::new();
    volume.set_liveness(Some(ConnectionLiveness::Dead));
    volume as Arc<dyn Volume>
}

impl Volume for ScriptedConnectionVolume {
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
    fn connection_liveness(&self) -> Option<ConnectionLiveness> {
        match self.liveness.load(Ordering::Relaxed) {
            1 => Some(ConnectionLiveness::Alive),
            2 => Some(ConnectionLiveness::Dead),
            _ => None,
        }
    }
    fn connection_bytes_received(&self) -> Option<u64> {
        match self.bytes_received.load(Ordering::Relaxed) {
            NO_COUNT => None,
            n => Some(n),
        }
    }
}
