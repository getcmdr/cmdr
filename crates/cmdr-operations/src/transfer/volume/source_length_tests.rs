//! A source stream that doesn't deliver the length it declared fails the file
//! instead of landing it (#538).
//!
//! `StreamLength::Known(n)` promises exactly `n` bytes. A source that ends short
//! (a download whose body was cut off by a proxy that also hid the length) or
//! runs long (a log still being written) breaks that promise, and a destination
//! that just copies until EOF would land a file of the wrong size under the
//! user's name, report it done, and on a move delete the original.
//!
//! The double declares a length other than the bytes it holds; everything else
//! is an ordinary `InMemoryVolume`.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::{copy_volumes_with_progress, forward_volume_methods, move_volumes_with_progress};
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::{VolumeCopyConfig, WriteOperationError};
use cmdr_fs::metadata::{CarriedMetadata, MetadataClasses};
use cmdr_fs::volume::{InMemoryVolume, StreamLength, Volume, VolumeError, VolumeReadStream};

/// A source whose streams all declare `declared` bytes, whatever they hold.
struct MisdeclaringSource {
    inner: Arc<InMemoryVolume>,
    declared: u64,
}

impl Volume for MisdeclaringSource {
    forward_volume_methods!(
        inner => name,
        root,
        lane_key,
        list_directory,
        get_metadata,
        exists,
        is_directory,
        create_file,
        create_directory,
        create_directory_all,
        delete,
        rename,
        get_space_info,
        local_path,
        supports_streaming,
        supports_export,
        supports_local_fs_access,
        operations_are_local,
        max_concurrent_ops,
        create_directory_errors_on_existing_dir,
        scan_for_copy,
        scan_for_copy_batch,
        scan_for_conflicts,
    );

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let inner = self.inner.open_read_stream(path).await?;
            Ok(Box::new(Misdeclared {
                inner,
                declared: self.declared,
            }) as Box<dyn VolumeReadStream>)
        })
    }
}

/// The inner stream's bytes under someone else's length.
struct Misdeclared {
    inner: Box<dyn VolumeReadStream>,
    declared: u64,
}

impl VolumeReadStream for Misdeclared {
    fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
        self.inner.next_chunk()
    }

    fn total_size(&self) -> StreamLength {
        StreamLength::Known(self.declared)
    }

    fn bytes_read(&self) -> u64 {
        self.inner.bytes_read()
    }

    fn modified_at(&self) -> Option<std::time::SystemTime> {
        self.inner.modified_at()
    }

    fn metadata(&self, wanted: MetadataClasses) -> CarriedMetadata {
        self.inner.metadata(wanted)
    }
}

const BYTES: &[u8] = b"the first six";

async fn source_declaring(declared: u64) -> (Arc<InMemoryVolume>, Arc<MisdeclaringSource>) {
    let inner = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    inner.create_file(app_path!("/log.txt"), BYTES).await.unwrap();
    let source = Arc::new(MisdeclaringSource {
        inner: Arc::clone(&inner),
        declared,
    });
    (inner, source)
}

async fn empty_dest() -> Arc<InMemoryVolume> {
    let dest = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    dest.create_directory(app_path!("/inbox")).await.unwrap();
    dest
}

async fn run(is_move: bool, source: Arc<MisdeclaringSource>, dest: Arc<InMemoryVolume>) -> WriteOperationError {
    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let events = Arc::new(CollectorEventSink::new());
    let paths = [PathBuf::from("/log.txt")];
    let config = VolumeCopyConfig::default();
    let source = source as Arc<dyn Volume>;
    let dest = dest as Arc<dyn Volume>;
    let result = if is_move {
        move_volumes_with_progress(
            events,
            "op-src-len",
            &state,
            source,
            &crate::testing::app_paths_of(&paths),
            dest,
            &crate::testing::app_path_of(Path::new("/inbox")),
            &config,
        )
        .await
    } else {
        copy_volumes_with_progress(
            events,
            "op-src-len",
            &state,
            source,
            &crate::testing::app_paths_of(&paths),
            dest,
            &crate::testing::app_path_of(Path::new("/inbox")),
            &config,
        )
        .await
    };
    result
        .expect_err("a source that broke its declared length must fail the file")
        .error
}

async fn assert_nothing_landed(dest: &InMemoryVolume) {
    let left = dest.list_directory(app_path!("/inbox"), None).await.unwrap();
    assert!(left.is_empty(), "no file and no staged temp may stay behind: {left:?}");
}

fn assert_source_changed(error: &WriteOperationError) {
    assert!(
        matches!(error, WriteOperationError::SourceChanged { path } if path == "/log.txt"),
        "a length the source didn't deliver is the source changing under the copy: {error:?}"
    );
}

/// The source ends short of what it declared: nothing lands.
#[tokio::test]
async fn a_copy_whose_source_ends_short_lands_nothing() {
    let (_, source) = source_declaring(BYTES.len() as u64 + 4).await;
    let dest = empty_dest().await;

    let error = run(false, source, Arc::clone(&dest)).await;

    assert_source_changed(&error);
    assert_nothing_landed(&dest).await;
}

/// The source outruns what it declared (a file still growing): nothing lands.
#[tokio::test]
async fn a_copy_whose_source_runs_long_lands_nothing() {
    let (_, source) = source_declaring(4).await;
    let dest = empty_dest().await;

    let error = run(false, source, Arc::clone(&dest)).await;

    assert_source_changed(&error);
    assert_nothing_landed(&dest).await;
}

/// CRITICAL data-loss regression (#538): a move whose copy came up short must
/// keep its original, since the copy is the one that's incomplete.
#[tokio::test]
async fn a_move_whose_source_ends_short_keeps_the_original() {
    let (inner, source) = source_declaring(BYTES.len() as u64 + 4).await;
    let dest = empty_dest().await;

    let error = run(true, source, Arc::clone(&dest)).await;

    assert_source_changed(&error);
    assert_nothing_landed(&dest).await;
    assert!(
        inner.exists(app_path!("/log.txt")).await,
        "the original must survive a move whose copy was short"
    );
}
