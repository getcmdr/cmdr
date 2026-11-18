//! The scan preview lifecycle API: what `get_scan_preview_totals` answers.

use crate::testing::TestOperations;
use std::path::PathBuf;

use uuid::Uuid;

use super::state::{CachedScanResult, FileInfo};
use cmdr_fs::volume::CopyScanResult;

/// `get_scan_preview_totals` must return the cached counters when a scan
/// has completed. Pins the contract the FE relies on to recover its
/// display state when scan events fire before listeners attach (the
/// regression that flaked `mtp-copy-preflight-uses-cache.spec.ts` after
/// the watcher-backed oracle made scans nearly instant).
#[test]
fn get_scan_preview_totals_returns_cached_counts_after_complete() {
    let engine = TestOperations::new();
    let preview_id = format!("test-{}", Uuid::new_v4());
    let source = PathBuf::from("/src");
    // A real local-walk shape: seven files, two directories, and one
    // per-source result. Building it through `from_local_walk` is what
    // keeps the fixture honest — `file_count` comes from the file list, so
    // the test can't quietly assert a count no walk would emit.
    let files: Vec<FileInfo> = (0..7)
        .map(|i| FileInfo {
            path: source.join(format!("f{i}.bin")),
            source_root: PathBuf::from("/"),
            size: 1_763,
            progress_bytes: 1_763,
            is_symlink: false,
            tree_node: None,
        })
        .collect();
    engine.stores().previews().insert_scan_result(
        preview_id.clone(),
        CachedScanResult::from_local_walk(
            crate::types::ScanPreviewPurpose::Transfer,
            vec![source.clone()],
            files,
            vec![source.join("d1"), source.join("d2")],
            12_345,
            12_345,
            vec![(
                source,
                CopyScanResult {
                    file_count: 7,
                    dir_count: 2,
                    total_bytes: 12_345,
                    dedup_bytes: 12_345,
                    top_level_is_directory: true,
                    top_level_modified_at: None,
                    top_level_created_at: None,
                },
            )],
            None,
        ),
    );

    let totals = engine
        .get_scan_preview_totals(&preview_id)
        .expect("totals should be present");
    assert_eq!(totals.files_total, 7);
    assert_eq!(totals.dirs_total, 2);
    assert_eq!(totals.bytes_total, 12_345);

    engine.stores().previews().release_preview(&preview_id);
}

/// A transfer's walk counts the names Windows can't use, from the scan, so the
/// dialog can warn before any byte moves; the recovery path carries the same
/// tally. Folder names count too, the selected source's own included.
/// Unix-only: the fixture's names are the ones Windows refuses to create.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_transfer_scan_counts_the_names_windows_cant_use() {
    use super::event_sinks::CollectorScanPreviewSink;
    use crate::types::ScanPreviewPurpose;
    use cmdr_fs::testing::{TestDir, wait_until_async};
    use std::sync::Arc;
    use std::time::Duration;

    let engine = TestOperations::new();
    engine.register_local_root();
    let dir = TestDir::new("scan_name_tally");
    let source = dir.join("12:00 notes");
    std::fs::create_dir_all(source.join("sub")).expect("folders");
    for name in ["a?b.txt", "con.txt", "trail.", "fine.txt"] {
        std::fs::write(source.join("sub").join(name), b"x").expect("a file");
    }
    let sink = Arc::new(CollectorScanPreviewSink::new());
    let started = engine
        .start_scan_preview(
            Arc::clone(&sink) as Arc<dyn super::event_sinks::ScanPreviewEventSink>,
            vec![source],
            None,
            "root".into(),
            50,
            ScanPreviewPurpose::Transfer,
            cmdr_fs::mac_only_files::MacOnlyFiles::Include,
        )
        .expect("the scan starts");
    wait_until_async(Duration::from_secs(10), "the scan's complete event", || {
        !sink.complete.lock().expect("collector mutex").is_empty()
    })
    .await;

    let tally = sink.complete.lock().expect("collector mutex")[0]
        .name_portability
        .clone()
        .expect("a transfer's scan carries a tally");
    assert_eq!(tally.windows_characters.count, 2, "{tally:?}");
    assert_eq!(tally.reserved_names.examples, ["con.txt"]);
    assert_eq!(tally.ends_with_space_or_period.examples, ["trail."]);
    let totals = engine.get_scan_preview_totals(&started.preview_id).expect("the totals");
    assert_eq!(totals.name_portability, Some(tally));
    engine.cancel_scan_preview(&started.preview_id);
}

/// `get_scan_preview_totals` returns `None` while the scan is still
/// running (the cache is keyed by preview_id; absence == not complete).
#[test]
fn get_scan_preview_totals_returns_none_for_unknown_preview() {
    let engine = TestOperations::new();
    let unknown = format!("nonexistent-{}", Uuid::new_v4());
    assert!(engine.get_scan_preview_totals(&unknown).is_none());
}
