//! Config defaults, the skipped-count suffix, and the scan phase's
//! `scan_for_copy_batch` progress callback.

use super::*;
use crate::testing::TestOperations;
use cmdr_fs::app_path;

#[test]
fn test_volume_copy_config_default() {
    let config = VolumeCopyConfig::default();
    assert_eq!(config.progress_interval_ms, 200);
    assert_eq!(config.max_conflicts_to_show, 100);
}

#[test]
fn test_format_skipped_suffix_zero_is_empty() {
    // The annotation is only present when something was actually skipped, so
    // the happy-path completion log stays terse.
    assert_eq!(format_skipped_suffix(0, 0), "");
    // Stray byte count without any files: still empty (treat files as the
    // truth, bytes is just metadata).
    assert_eq!(format_skipped_suffix(0, 12345), "");
}

#[test]
fn test_format_skipped_suffix_singular() {
    assert_eq!(format_skipped_suffix(1, 0), " (of which skipped 1 file, 0 B)");
    // Humanized via search::query::format_size (base 1024, so IEC symbols).
    assert_eq!(
        format_skipped_suffix(1, 3_100_000_000),
        " (of which skipped 1 file, 2.9 GiB)"
    );
}

#[test]
fn test_format_skipped_suffix_plural() {
    assert_eq!(format_skipped_suffix(2, 200), " (of which skipped 2 files, 200 B)");
    assert_eq!(
        format_skipped_suffix(821, 17_500_000_000),
        " (of which skipped 821 files, 16.3 GiB)"
    );
}

/// A volume copy's own scan records every entry into the operation's live tree.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_volume_copy_hands_its_operation_the_tree_its_scan_recorded() {
    use crate::transfer_tree::test_support::outline;

    let engine = TestOperations::new();

    let (source, dest) = make_volumes();
    source.create_directory(app_path!("/src/photos/2024")).await.unwrap();
    source
        .create_file(app_path!("/src/photos/a.jpg"), b"aaa")
        .await
        .unwrap();
    source
        .create_file(app_path!("/src/photos/2024/b.jpg"), b"bbbbb")
        .await
        .unwrap();
    source.create_file(app_path!("/src/notes.txt"), b"nn").await.unwrap();
    let state = make_state(&engine);

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-volume-copy-tree",
        &state,
        Arc::clone(&source),
        &[PathBuf::from("/src/photos"), PathBuf::from("/src/notes.txt")],
        Arc::clone(&dest),
        Path::new("/"),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("the copy succeeds");

    let tree = state.transfer_tree.get().expect("the scan installed a tree");
    assert_eq!(
        tree.read(outline).expect("the tree is on"),
        vec![
            "photos/ 2 files 8 B",
            "  2024/ 1 files 5 B",
            "    b.jpg 5 B",
            "  a.jpg 3 B",
            "notes.txt 2 B",
        ]
    );
}

/// A cached preview that built no tree still tells the operation what each
/// source is: files, and folders whose contents the drivers list as they go.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cached_preview_without_a_tree_gives_the_operation_its_sources_as_opaque_rows() {
    use crate::state::CachedScanResult;
    use crate::transfer_tree::test_support::outline;
    use cmdr_fs::volume::CopyScanResult;

    let engine = TestOperations::new();

    let (source, dest) = make_volumes();
    source.create_directory(app_path!("/src/photos")).await.unwrap();
    source
        .create_file(app_path!("/src/photos/a.jpg"), b"aaa")
        .await
        .unwrap();
    source.create_file(app_path!("/src/notes.txt"), b"nn").await.unwrap();
    let scan = |files: usize, bytes: u64, is_directory: bool| CopyScanResult {
        file_count: files,
        dir_count: 0,
        total_bytes: bytes,
        dedup_bytes: bytes,
        top_level_is_directory: is_directory,
        top_level_modified_at: None,
        top_level_created_at: None,
    };
    let sources = vec![PathBuf::from("/src/photos"), PathBuf::from("/src/notes.txt")];
    let preview_id = format!("tree-opaque-{}", std::process::id());
    engine.stores().previews().insert_scan_result(
        preview_id.clone(),
        CachedScanResult::from_volume_batch(
            crate::types::ScanPreviewPurpose::Transfer,
            sources.clone(),
            2,
            5,
            5,
            vec![
                (sources[0].clone(), scan(1, 3, true)),
                (sources[1].clone(), scan(1, 2, false)),
            ],
        ),
    );
    let state = make_state(&engine);

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "op-volume-copy-opaque-tree",
        &state,
        Arc::clone(&source),
        &sources,
        Arc::clone(&dest),
        Path::new("/"),
        &VolumeCopyConfig {
            preview_id: Some(preview_id),
            ..VolumeCopyConfig::default()
        },
    )
    .await
    .expect("the copy succeeds");

    // The folder stays opaque (its totals are the scan's), and the copy's walk
    // fills in the children it met.
    let tree = state.transfer_tree.get().expect("the cached preview installed a tree");
    assert_eq!(
        tree.read(outline).expect("the tree is on"),
        vec!["photos/? 1 files 3 B", "  a.jpg 3 B", "notes.txt 2 B"]
    );
}

// ========================================
// LocalPosixVolume integration tests
// ========================================

/// `scan_for_copy_batch_with_progress` must invoke the callback as it discovers
/// entries so the FE's scan-preview dialog can show a climbing count instead of
/// a frozen 0/0/0 spinner. The default trait implementation (used by
/// `InMemoryVolume` and `LocalPosixVolume`) fires the callback once per scanned
/// path with the running total; `MtpVolume` overrides to thread it through
/// `list_directory_with_progress` for per-entry granularity.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_scan_for_copy_batch_with_progress_fires_callback() {
    use std::sync::Mutex;

    let vol = InMemoryVolume::new("V").with_space_info(1_000_000, 500_000);
    vol.create_file(app_path!("/a.txt"), b"AA").await.unwrap();
    vol.create_file(app_path!("/b.txt"), b"BBBB").await.unwrap();
    vol.create_file(app_path!("/c.txt"), b"CCCCCC").await.unwrap();
    let vol: Arc<dyn Volume> = Arc::new(vol);

    let calls = Arc::new(Mutex::new(Vec::<usize>::new()));
    let calls_for_cb = Arc::clone(&calls);
    let on_progress = move |p: ListingProgress| {
        calls_for_cb.lock().unwrap().push(p.files);
    };

    let paths = vec![
        PathBuf::from("/a.txt"),
        PathBuf::from("/b.txt"),
        PathBuf::from("/c.txt"),
    ];
    let boundary = cmdr_fs::volume::ScanBoundary::new(Some(&on_progress));
    let result = vol
        .scan_for_copy_batch_with_boundary(
            &paths
                .iter()
                .map(|p| AppPath::legacy_unchecked(p).into_owned())
                .collect::<Vec<_>>(),
            &boundary,
        )
        .await
        .unwrap();

    assert_eq!(result.aggregate.file_count, 3);
    assert_eq!(result.aggregate.total_bytes, 12); // 2 + 4 + 6

    // Callback must have fired with a monotonically growing count, ending at 3.
    let recorded = calls.lock().unwrap();
    assert!(!recorded.is_empty(), "on_progress must fire at least once");
    assert!(
        recorded.windows(2).all(|w| w[0] <= w[1]),
        "progress counts must be monotonic; saw {recorded:?}",
    );
    assert_eq!(
        *recorded.last().unwrap(),
        3,
        "final progress callback should report the full file count",
    );
}

/// Backwards-compat: the no-progress `scan_for_copy_batch` must keep working
/// (it's still called by `copy_volumes_with_progress`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_scan_for_copy_batch_without_progress_still_works() {
    let vol = InMemoryVolume::new("V").with_space_info(1_000_000, 500_000);
    vol.create_file(app_path!("/x.txt"), b"hello").await.unwrap();
    let vol: Arc<dyn Volume> = Arc::new(vol);

    let result = vol
        .scan_for_copy_batch(&[AppPath::legacy_unchecked(&PathBuf::from("/x.txt")).into_owned()])
        .await
        .unwrap();
    assert_eq!(result.aggregate.file_count, 1);
    assert_eq!(result.aggregate.total_bytes, 5);
}
