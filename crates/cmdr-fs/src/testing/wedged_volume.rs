//! [`WedgedVolume`]: a volume that never answers, for every bound that exists to
//! survive a wedged mount. Shared because the app's tests and the write
//! engine's both drive one.

use crate::app_path::AppPath;
use crate::app_path::AppPathBuf;
use std::future::Future;
use std::pin::Pin;

use crate::entry::FileEntry;
use crate::volume::{
    BatchScanResult, CopyScanResult, InMemoryVolume, ListingProgress, ScanConflict, SourceItemInfo, Volume, VolumeError,
};

/// A volume that never answers: every read and scan future parks forever.
///
/// This is what a wedged mount looks like from inside the app: no error, no
/// cancel, no progress, no return. It's the fixture for every bound that exists
/// to survive one, because a real network drop isn't repeatable and a volume
/// that never answers is exactly repeatable and reaches the same code.
///
/// Name and root come from an `InMemoryVolume` so this is a real `Volume` rather
/// than a panic trap.
pub struct WedgedVolume {
    inner: InMemoryVolume,
}

impl WedgedVolume {
    /// A wedged volume named `name`.
    pub fn new(name: &str) -> Self {
        Self {
            inner: InMemoryVolume::new(name),
        }
    }
}

/// Every wedged method body: park, and never come back.
macro_rules! never_answers {
    () => {
        Box::pin(async move {
            std::future::pending::<()>().await;
            unreachable!("a wedged volume never answers")
        })
    };
}

impl Volume for WedgedVolume {
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
        _on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        never_answers!()
    }

    fn get_metadata<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        never_answers!()
    }

    fn exists<'a>(&'a self, _path: &'a AppPath) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        never_answers!()
    }

    fn is_directory<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        never_answers!()
    }

    fn read_range<'a>(
        &'a self,
        _path: &'a AppPath,
        _offset: u64,
        _len: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, VolumeError>> + Send + 'a>> {
        never_answers!()
    }

    fn scan_for_copy<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<CopyScanResult, VolumeError>> + Send + 'a>> {
        never_answers!()
    }

    fn scan_for_copy_batch_with_boundary<'a>(
        &'a self,
        _paths: &'a [AppPathBuf],
        _boundary: &'a crate::volume::ScanBoundary<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<BatchScanResult, VolumeError>> + Send + 'a>> {
        never_answers!()
    }

    fn scan_for_conflicts<'a>(
        &'a self,
        _items: &'a [SourceItemInfo],
        _dest: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ScanConflict>, VolumeError>> + Send + 'a>> {
        never_answers!()
    }
}
