//! Which side a failure deep in a folder transfer is reported against.
//!
//! cmdr-reports#62 (`ERR-UK5NH`): a local folder moved onto an SMB share, and
//! the share refused to create a NESTED folder (`STATUS_ACCESS_DENIED`). The
//! walker labelled that refusal with the SOURCE item it was on, so the dialog
//! used source-side move wording and told the user to unlock their intact local
//! folder in Finder. The refusal was the destination's, and so is the path the
//! user needs to see.
//!
//! The rule these cells pin, for the serial copy, the concurrent copy, and the
//! cross-volume move: a refusal from the DESTINATION (a nested mkdir, a file
//! write, the landing rename) is reported as the destination's, at the
//! destination path; a refusal from the SOURCE (opening a file, reading it) is
//! still the source's, at the source path, which is what cmdr-reports#17 needs
//! (`move_failure_tests.rs::cross_volume_move_that_cannot_remove_the_original_says_the_copy_landed`).
//!
//! The source lives at `/tN/...` and the destination at `/inbox/tN/...`, so the
//! reported path alone tells the two sides apart.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::faulty_volume::{FaultyOp, FaultyVolume};
use super::{copy_volumes_with_progress, move_volumes_with_progress};
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::{PermissionSide, VolumeCopyConfig, WriteOperationError};
use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError};

#[derive(Clone, Copy, Debug)]
enum Driver {
    /// One source, so `copy.rs` takes the serial driver.
    SerialCopy,
    /// Three sources, so `copy.rs` takes the concurrent driver.
    ConcurrentCopy,
    /// `move_cross.rs`, one source.
    CrossVolumeMove,
}

impl Driver {
    fn sources(self) -> usize {
        match self {
            Self::ConcurrentCopy => 3,
            Self::SerialCopy | Self::CrossVolumeMove => 1,
        }
    }
}

fn refused(path: &str) -> VolumeError {
    VolumeError::PermissionDenied {
        path: path.to_string(),
        raw_os_error: None,
    }
}

/// `/tN/nested/deep.txt` for each source, and the paths to transfer.
async fn seeded_source(count: usize) -> (Arc<InMemoryVolume>, Vec<PathBuf>) {
    let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    let mut paths = Vec::new();
    for n in 0..count {
        let root = PathBuf::from(format!("/t{n}"));
        source
            .create_directory(&AppPath::legacy_unchecked(&root))
            .await
            .unwrap();
        source
            .create_directory(&AppPath::legacy_unchecked(&root.join("nested")))
            .await
            .unwrap();
        source
            .create_file(
                &AppPath::legacy_unchecked(&root.join("nested/deep.txt")),
                b"the user's bytes",
            )
            .await
            .unwrap();
        paths.push(root);
    }
    (source, paths)
}

async fn empty_dest() -> Arc<InMemoryVolume> {
    let dest = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    dest.create_directory(app_path!("/inbox")).await.unwrap();
    dest
}

async fn run(
    engine: &TestOperations,
    driver: Driver,
    source: Arc<dyn Volume>,
    paths: &[PathBuf],
    dest: Arc<dyn Volume>,
) -> Result<(), WriteOperationError> {
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let events = Arc::new(CollectorEventSink::new());
    let config = VolumeCopyConfig::default();
    let result = match driver {
        Driver::SerialCopy | Driver::ConcurrentCopy => {
            copy_volumes_with_progress(
                events,
                "op-failure-side",
                &state,
                source,
                paths,
                dest,
                Path::new("/inbox"),
                &config,
            )
            .await
        }
        Driver::CrossVolumeMove => {
            move_volumes_with_progress(
                events,
                "op-failure-side",
                &state,
                source,
                paths,
                dest,
                Path::new("/inbox"),
                &config,
            )
            .await
        }
    };
    result.map(|_| ()).map_err(|failure| failure.error)
}

/// Runs `driver` against a destination armed with `op` failing on its `nth`
/// call, and checks the refusal comes back as the destination's, at a
/// destination path ending in `ends_with`.
async fn assert_destination_refusal(
    engine: &TestOperations,
    driver: Driver,
    op: FaultyOp,
    nth: usize,
    ends_with: &str,
) {
    let (source, paths) = seeded_source(driver.sources()).await;
    let dest = FaultyVolume::wrapping(empty_dest().await)
        .failing_call(op, nth, refused("/the share's own words"))
        .arc();

    let result = run(
        engine,
        driver,
        source.clone(),
        &paths,
        Arc::clone(&dest) as Arc<dyn Volume>,
    )
    .await;

    assert!(
        dest.fault_fired(op),
        "{driver:?}: the injected {op:?} never fired, so this cell proves nothing"
    );
    let error = result.expect_err("a destination that refuses must fail the transfer");
    let WriteOperationError::PermissionDenied { path, side, .. } = &error else {
        panic!("{driver:?} {op:?}: expected PermissionDenied, got {error:?}");
    };
    assert_eq!(
        *side,
        Some(PermissionSide::Destination),
        "{driver:?} {op:?}: the destination refused, so it's the destination's refusal: {error:?}"
    );
    assert!(
        path.starts_with("/inbox/t") && path.ends_with(ends_with),
        "{driver:?} {op:?}: the report names the DESTINATION item that refused, got {path:?}"
    );
    // A move that failed in its copy phase never touches the source.
    for path in &paths {
        assert!(
            source
                .exists(&AppPath::legacy_unchecked(&path.join("nested/deep.txt")))
                .await
        );
    }
}

/// Runs `driver` against a source armed with `op` failing on its first call,
/// and checks the failure stays the source's, at the source file's path.
async fn assert_source_refusal(engine: &TestOperations, driver: Driver, op: FaultyOp) {
    let (source, paths) = seeded_source(driver.sources()).await;
    let source = FaultyVolume::wrapping(source)
        .failing_call(op, 1, refused("/the source's own words"))
        .arc();

    let result = run(
        engine,
        driver,
        Arc::clone(&source) as Arc<dyn Volume>,
        &paths,
        empty_dest().await as Arc<dyn Volume>,
    )
    .await;

    assert!(
        source.fault_fired(op),
        "{driver:?}: the injected {op:?} never fired, so this cell proves nothing"
    );
    let error = result.expect_err("a source that refuses must fail the transfer");
    let WriteOperationError::PermissionDenied { path, side, .. } = &error else {
        panic!("{driver:?} {op:?}: expected PermissionDenied, got {error:?}");
    };
    assert_eq!(
        *side,
        Some(PermissionSide::Source),
        "{driver:?} {op:?}: the source refused, so it stays the source's refusal: {error:?}"
    );
    assert!(
        path.starts_with("/t") && path.ends_with("/nested/deep.txt"),
        "{driver:?} {op:?}: the report names the SOURCE file that refused, got {path:?}"
    );
}

// A nested mkdir. The serial drivers count `create_directory_all("/inbox")`
// first, then `/inbox/t0`, so the third call is `/inbox/t0/nested`: the
// report's exact shape. The concurrent driver interleaves its sources, so any
// folder under `/inbox` will do there.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_reports_a_refused_nested_folder_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(&engine, Driver::SerialCopy, FaultyOp::CreateDirectory, 3, "/nested").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_reports_a_refused_nested_folder_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(&engine, Driver::ConcurrentCopy, FaultyOp::CreateDirectory, 5, "").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_reports_a_refused_nested_folder_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(
        &engine,
        Driver::CrossVolumeMove,
        FaultyOp::CreateDirectory,
        3,
        "/nested",
    )
    .await;
}

// A file write deep in the tree.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_reports_a_refused_file_write_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(
        &engine,
        Driver::SerialCopy,
        FaultyOp::WriteFromStream,
        1,
        "/nested/deep.txt",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_reports_a_refused_file_write_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(
        &engine,
        Driver::ConcurrentCopy,
        FaultyOp::WriteFromStream,
        1,
        "/nested/deep.txt",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_reports_a_refused_file_write_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(
        &engine,
        Driver::CrossVolumeMove,
        FaultyOp::WriteFromStream,
        1,
        "/nested/deep.txt",
    )
    .await;
}

// The landing rename that gives a staged file its name.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_reports_a_refused_landing_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(&engine, Driver::SerialCopy, FaultyOp::Rename, 1, "/nested/deep.txt").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_reports_a_refused_landing_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(&engine, Driver::ConcurrentCopy, FaultyOp::Rename, 1, "/nested/deep.txt").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_reports_a_refused_landing_as_the_destinations() {
    let engine = TestOperations::new();
    assert_destination_refusal(
        &engine,
        Driver::CrossVolumeMove,
        FaultyOp::Rename,
        1,
        "/nested/deep.txt",
    )
    .await;
}

// The source side stays the source's: a file that won't open, and one that
// opens and then fails its read (which reaches the engine through the
// destination's `write_from_stream`).

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_reports_an_unopenable_source_file_as_the_sources() {
    let engine = TestOperations::new();
    assert_source_refusal(&engine, Driver::SerialCopy, FaultyOp::OpenReadStream).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_reports_an_unreadable_source_file_as_the_sources() {
    let engine = TestOperations::new();
    assert_source_refusal(&engine, Driver::SerialCopy, FaultyOp::ReadStream).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_reports_an_unreadable_source_file_as_the_sources() {
    let engine = TestOperations::new();
    assert_source_refusal(&engine, Driver::ConcurrentCopy, FaultyOp::ReadStream).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_reports_an_unopenable_source_file_as_the_sources() {
    let engine = TestOperations::new();
    assert_source_refusal(&engine, Driver::CrossVolumeMove, FaultyOp::OpenReadStream).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_reports_an_unreadable_source_file_as_the_sources() {
    let engine = TestOperations::new();
    assert_source_refusal(&engine, Driver::CrossVolumeMove, FaultyOp::ReadStream).await;
}
