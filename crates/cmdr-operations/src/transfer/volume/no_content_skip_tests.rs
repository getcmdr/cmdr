//! A child with no bytes to copy (`VolumeError::NoContent`: a symlink the source
//! can't recreate, a device, fifo, or socket) is ONE skipped item, ❌ never the
//! end of the folder it sits in. A restic backup of a Mac is full of symlinks,
//! so without this no folder of one would ever copy out.

use cmdr_fs::app_path;
use cmdr_fs::app_path::{AppPath, AppPathBuf, PathFlavor};

use std::sync::Arc;
use std::time::Duration;

use super::faulty_volume::{FaultyOp, FaultyVolume};
use super::{copy_volumes_with_progress, move_volumes_with_progress};
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::VolumeCopyConfig;
use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError};

/// `/tN/keep.txt` for each source, and `/t0/link`, the one with no bytes.
async fn seeded_source(count: usize) -> (Arc<InMemoryVolume>, Vec<AppPathBuf>) {
    let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    let mut paths = Vec::new();
    for n in 0..count {
        // Spelled as app text: a `std::path` join puts Windows' `\` into it, one name on the double.
        let top = AppPathBuf::parse(&format!("/t{n}"), PathFlavor::Posix).expect("an app path");
        let keep = AppPathBuf::parse(&format!("/t{n}/keep.txt"), PathFlavor::Posix).expect("an app path");
        source.create_directory(&top).await.unwrap();
        source.create_file(&keep, b"kept").await.unwrap();
        paths.push(top);
    }
    source
        .create_file(AppPath::from_normalized(LINK).expect("a canonical path"), b"")
        .await
        .unwrap();
    (source, paths)
}

const LINK: &str = "/t0/link";

/// The source, with `link` answering `NoContent` when opened.
fn without_bytes_at_link(source: Arc<InMemoryVolume>) -> Arc<FaultyVolume<InMemoryVolume>> {
    FaultyVolume::wrapping(source)
        .failing_call_at(
            FaultyOp::OpenReadStream,
            LINK,
            1,
            VolumeError::NoContent(LINK.to_string()),
        )
        .arc()
}

async fn empty_dest() -> Arc<InMemoryVolume> {
    let dest = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    dest.create_directory(app_path!("/inbox")).await.unwrap();
    dest
}

async fn copy_skips_and_carries_on(sources: usize) {
    let engine = TestOperations::new();
    let (inner, paths) = seeded_source(sources).await;
    let source = without_bytes_at_link(inner);
    let dest = empty_dest().await;
    let events = Arc::new(CollectorEventSink::new());

    let result = copy_volumes_with_progress(
        events.clone(),
        "op-no-content",
        &Arc::new(engine.new_operation_state(Duration::from_millis(50))),
        Arc::clone(&source) as Arc<dyn Volume>,
        &paths,
        Arc::clone(&dest) as Arc<dyn Volume>,
        app_path!("/inbox"),
        &VolumeCopyConfig::default(),
    )
    .await;

    assert!(
        source.fault_fired(FaultyOp::OpenReadStream),
        "the cell never met a link"
    );
    assert!(result.is_ok(), "a link doesn't stop the copy: {result:?}");
    for n in 0..sources {
        assert!(
            dest.exists(&AppPathBuf::parse(&format!("/inbox/t{n}/keep.txt"), PathFlavor::Posix).expect("an app path"))
                .await
        );
    }
    assert!(!dest.exists(app_path!("/inbox/t0/link")).await);
    let complete = events.complete.lock().unwrap();
    assert_eq!(complete[0].files_skipped, 1, "the link is one skipped item");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_skips_a_child_with_no_bytes_and_lands_the_rest() {
    copy_skips_and_carries_on(1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_skips_a_child_with_no_bytes_and_lands_the_rest() {
    copy_skips_and_carries_on(3).await;
}

/// A move's sweep spares what it skipped: the link never landed, so deleting it
/// would destroy the only copy.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_leaves_a_child_with_no_bytes_at_the_source() {
    let engine = TestOperations::new();
    let (inner, paths) = seeded_source(1).await;
    let source = without_bytes_at_link(Arc::clone(&inner));
    let dest = empty_dest().await;

    let result = move_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-no-content-move",
        &Arc::new(engine.new_operation_state(Duration::from_millis(50))),
        Arc::clone(&source) as Arc<dyn Volume>,
        &paths,
        Arc::clone(&dest) as Arc<dyn Volume>,
        app_path!("/inbox"),
        &VolumeCopyConfig::default(),
    )
    .await;

    assert!(result.is_ok(), "{result:?}");
    assert!(dest.exists(app_path!("/inbox/t0/keep.txt")).await);
    assert!(inner.exists(app_path!("/t0/link")).await, "the skipped link stays");
}
