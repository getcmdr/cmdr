//! A source listing one name twice: two same-named items in a phone or camera
//! folder (#523), or an archive holding one path twice.
//!
//! ❗ Regression anchor: a level this copy created counted every name as free
//! by proof, so the second item went for the name the first had just taken.
//! The landing refused it, and the whole copy stopped on a `DestinationExists`
//! no policy had been asked about. The second item is now a clash like any
//! other, settled by the conflict policy.
//!
//! The source lies through `InMemoryVolume::set_reported_name`: `b.jpg` lists
//! as `a.jpg` and still reads as itself, the way a device's object does.

use super::copy::copy_volumes_with_progress;
use super::rename_merge_test_support::make_state;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, VolumeCopyConfig};
use cmdr_fs::app_path;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_local::LocalVolume;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

const FIRST: &str = "FIRST";
const SECOND: &str = "SECOND";

/// Copies a source `/album` listing `a.jpg` twice into a fresh `/inbox` under
/// `policy`, and answers what landed in `inbox/album`, by content.
async fn copy_twins_under(policy: ConflictResolution) -> BTreeSet<String> {
    let engine = TestOperations::new();
    let source = InMemoryVolume::new("Phone").with_space_info(10_000_000, 10_000_000);
    source.create_directory(app_path!("/album")).await.unwrap();
    source
        .create_file(app_path!("/album/a.jpg"), FIRST.as_bytes())
        .await
        .unwrap();
    source
        .create_file(app_path!("/album/b.jpg"), SECOND.as_bytes())
        .await
        .unwrap();
    source.set_reported_name(Path::new("/album/b.jpg"), "a.jpg");
    let source: Arc<dyn Volume> = Arc::new(source);

    let dest_dir = TempDir::new().unwrap();
    std::fs::create_dir(dest_dir.path().join("inbox")).unwrap();
    let dest: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder(
        "Dest",
        dest_dir.path().to_path_buf(),
        crate::host::testing::pane_order_host(),
    ));

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "repeated-name-copy",
        &make_state(&engine),
        source,
        &crate::testing::app_paths_of(&[PathBuf::from("/album")]),
        dest,
        &crate::testing::app_path_of(Path::new("/inbox")),
        &VolumeCopyConfig {
            conflict_resolution: policy,
            progress_interval_ms: 0,
            ..VolumeCopyConfig::default()
        },
    )
    .await
    .unwrap_or_else(|failure| panic!("{policy:?}: the copy must settle the clash, got {:?}", failure.error));

    std::fs::read_dir(dest_dir.path().join("inbox/album"))
        .expect("the album landed")
        .map(|entry| std::fs::read_to_string(entry.expect("a dir entry").path()).expect("a landed file"))
        .collect()
}

#[cfg_attr(
    target_os = "windows",
    ignore = "#1225 milestone 10a: a native path's `\\` reaches the volume engine as an app path (\"a backslash in a drive or UNC path\")"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repeated_name_under_rename_lands_both_items() {
    assert_eq!(
        copy_twins_under(ConflictResolution::Rename).await,
        BTreeSet::from([FIRST.to_string(), SECOND.to_string()])
    );
}

#[cfg_attr(
    target_os = "windows",
    ignore = "#1225 milestone 10a: a native path's `\\` reaches the volume engine as an app path (\"a backslash in a drive or UNC path\")"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repeated_name_under_skip_lands_the_first_item_only() {
    let landed = copy_twins_under(ConflictResolution::Skip).await;
    assert_eq!(landed.len(), 1, "the second item is skipped, got {landed:?}");
}
