//! `InMemoryVolume`'s scan surface: `scan_for_copy`, the batch default the trait
//! provides, space info, and `scan_for_conflicts`. Core CRUD is
//! `in_memory_test.rs`; streaming is `in_memory_stream_test.rs`.

use super::*;
use crate::app_path;
use std::path::{Path, PathBuf};

// ============================================================================
// scan_for_copy tests
// ============================================================================

#[tokio::test]
async fn test_scan_for_copy_single_file() {
    let volume = InMemoryVolume::new("Test");
    volume
        .create_file(app_path!("/report.txt"), b"Hello, World!")
        .await
        .unwrap();

    let result = volume.scan_for_copy(app_path!("/report.txt")).await.unwrap();
    assert_eq!(result.file_count, 1);
    assert_eq!(result.dir_count, 0);
    assert_eq!(result.total_bytes, 13);
}

#[tokio::test]
async fn test_scan_for_copy_empty_directory() {
    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/empty")).await.unwrap();

    let result = volume.scan_for_copy(app_path!("/empty")).await.unwrap();
    assert_eq!(result.file_count, 0);
    assert_eq!(result.dir_count, 0);
    assert_eq!(result.total_bytes, 0);
}

#[tokio::test]
async fn test_scan_for_copy_directory_with_files() {
    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/docs")).await.unwrap();
    volume
        .create_file(app_path!("/docs/readme.txt"), b"Read me")
        .await
        .unwrap();
    volume
        .create_file(app_path!("/docs/notes.txt"), b"Notes here")
        .await
        .unwrap();

    let result = volume.scan_for_copy(app_path!("/docs")).await.unwrap();
    assert_eq!(result.file_count, 2);
    assert_eq!(result.dir_count, 0);
    assert_eq!(result.total_bytes, 17); // 7 + 10
}

#[tokio::test]
async fn test_scan_for_copy_nested_directory_tree() {
    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/root")).await.unwrap();
    volume.create_directory(app_path!("/root/sub")).await.unwrap();
    volume.create_directory(app_path!("/root/sub/deep")).await.unwrap();
    volume.create_file(app_path!("/root/file1.txt"), b"AAA").await.unwrap();
    volume
        .create_file(app_path!("/root/sub/file2.txt"), b"BBBBB")
        .await
        .unwrap();
    volume
        .create_file(app_path!("/root/sub/deep/file3.txt"), b"C")
        .await
        .unwrap();

    let result = volume.scan_for_copy(app_path!("/root")).await.unwrap();
    assert_eq!(result.file_count, 3);
    assert_eq!(result.dir_count, 2); // sub + deep (root not counted)
    assert_eq!(result.total_bytes, 9); // 3 + 5 + 1
}

// ============================================================================
// scan_for_copy_batch tests (default implementation via Volume trait)
// ============================================================================

#[tokio::test]
async fn test_scan_for_copy_batch_multiple_files_same_dir() {
    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/photos")).await.unwrap();
    volume.create_file(app_path!("/photos/a.jpg"), &[0; 100]).await.unwrap();
    volume.create_file(app_path!("/photos/b.jpg"), &[0; 200]).await.unwrap();
    volume.create_file(app_path!("/photos/c.jpg"), &[0; 300]).await.unwrap();

    let paths = [
        PathBuf::from("/photos/a.jpg"),
        PathBuf::from("/photos/b.jpg"),
        PathBuf::from("/photos/c.jpg"),
    ];
    let result = volume
        .scan_for_copy_batch(
            &paths
                .iter()
                .map(|p| AppPath::legacy_unchecked(p).into_owned())
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
    assert_eq!(result.aggregate.file_count, 3);
    assert_eq!(result.aggregate.dir_count, 0);
    assert_eq!(result.aggregate.total_bytes, 600);
    assert_eq!(result.per_path.len(), 3);
}

#[tokio::test]
async fn test_scan_for_copy_batch_mixed_files_and_dirs() {
    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/stuff")).await.unwrap();
    volume
        .create_file(app_path!("/stuff/readme.txt"), b"hello")
        .await
        .unwrap();
    volume.create_directory(app_path!("/stuff/subdir")).await.unwrap();
    volume
        .create_file(app_path!("/stuff/subdir/deep.txt"), &[0; 50])
        .await
        .unwrap();

    let paths = [PathBuf::from("/stuff/readme.txt"), PathBuf::from("/stuff/subdir")];
    let result = volume
        .scan_for_copy_batch(
            &paths
                .iter()
                .map(|p| AppPath::legacy_unchecked(p).into_owned())
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
    assert_eq!(result.aggregate.file_count, 2); // readme.txt + deep.txt
    assert_eq!(result.aggregate.dir_count, 0); // subdir's children don't include extra dirs
    assert_eq!(result.aggregate.total_bytes, 55); // 5 + 50
    assert_eq!(result.per_path.len(), 2);
    // The file entry should report top_level_is_directory=false; the dir one true.
    let readme = result
        .per_path
        .iter()
        .find(|(p, _)| p.as_std_legacy() == Path::new("/stuff/readme.txt"))
        .unwrap();
    assert!(!readme.1.top_level_is_directory);
    assert_eq!(readme.1.total_bytes, 5);
    let subdir = result
        .per_path
        .iter()
        .find(|(p, _)| p.as_std_legacy() == Path::new("/stuff/subdir"))
        .unwrap();
    assert!(subdir.1.top_level_is_directory);
}

/// The double hands a recorder its whole selection, entry by entry, so a
/// transfer's tree over it is as complete as over any walking backend.
#[tokio::test]
async fn a_batch_scan_records_every_entry_under_the_callers_paths() {
    use crate::testing::scan_recording::{Recorded, ScanRecording};
    use std::sync::Arc;

    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/stuff")).await.unwrap();
    volume
        .create_file(app_path!("/stuff/readme.txt"), b"hello")
        .await
        .unwrap();
    volume.create_directory(app_path!("/stuff/subdir")).await.unwrap();
    volume.create_directory(app_path!("/stuff/subdir/empty")).await.unwrap();
    volume
        .create_file(app_path!("/stuff/subdir/deep.txt"), &[0; 50])
        .await
        .unwrap();
    let recording = Arc::new(ScanRecording::default());
    let boundary = ScanBoundary::silent().recording_to(Arc::clone(&recording) as Arc<dyn ScanRecorder>);

    let paths = [PathBuf::from("/stuff/readme.txt"), PathBuf::from("/stuff/subdir")];
    let result = volume
        .scan_for_copy_batch_with_boundary(
            &paths
                .iter()
                .map(|p| AppPath::legacy_unchecked(p).into_owned())
                .collect::<Vec<_>>(),
            &boundary,
        )
        .await
        .unwrap();

    assert_eq!(result.aggregate.file_count, 2, "the totals don't change");
    assert_eq!(
        recording.sorted(),
        vec![
            Recorded::Dir("/stuff/subdir".into()),
            Recorded::Dir("/stuff/subdir/empty".into()),
            Recorded::File("/stuff/readme.txt".into(), 5),
            Recorded::File("/stuff/subdir/deep.txt".into(), 50),
        ]
    );
}

#[tokio::test]
async fn test_scan_for_copy_batch_empty_input() {
    let volume = InMemoryVolume::new("Test");
    let result = volume.scan_for_copy_batch(&[]).await.unwrap();
    assert_eq!(result.aggregate.file_count, 0);
    assert_eq!(result.aggregate.dir_count, 0);
    assert_eq!(result.aggregate.total_bytes, 0);
    assert!(result.per_path.is_empty());
}

#[tokio::test]
async fn test_scan_for_copy_batch_single_item_matches_single_scan() {
    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/docs")).await.unwrap();
    volume.create_file(app_path!("/docs/a.txt"), b"data").await.unwrap();

    let single = volume.scan_for_copy(app_path!("/docs/a.txt")).await.unwrap();
    let batch = volume
        .scan_for_copy_batch(&[AppPath::legacy_unchecked(&PathBuf::from("/docs/a.txt")).into_owned()])
        .await
        .unwrap();
    assert_eq!(single.file_count, batch.aggregate.file_count);
    assert_eq!(single.dir_count, batch.aggregate.dir_count);
    assert_eq!(single.total_bytes, batch.aggregate.total_bytes);
    assert_eq!(batch.per_path.len(), 1);
}

#[tokio::test]
async fn test_scan_for_copy_batch_files_from_different_dirs() {
    let volume = InMemoryVolume::new("Test");
    volume.create_directory(app_path!("/a")).await.unwrap();
    volume.create_directory(app_path!("/b")).await.unwrap();
    volume.create_file(app_path!("/a/file1.txt"), &[0; 10]).await.unwrap();
    volume.create_file(app_path!("/b/file2.txt"), &[0; 20]).await.unwrap();

    let paths = [PathBuf::from("/a/file1.txt"), PathBuf::from("/b/file2.txt")];
    let result = volume
        .scan_for_copy_batch(
            &paths
                .iter()
                .map(|p| AppPath::legacy_unchecked(p).into_owned())
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
    assert_eq!(result.aggregate.file_count, 2);
    assert_eq!(result.aggregate.total_bytes, 30);
    assert_eq!(result.per_path.len(), 2);
}

// ============================================================================
// get_space_info tests
// ============================================================================

#[tokio::test]
async fn test_get_space_info_not_supported_by_default() {
    let volume = InMemoryVolume::new("Test");
    assert!(matches!(volume.get_space_info().await, Err(VolumeError::NotSupported)));
}

#[tokio::test]
async fn test_get_space_info_with_configured_space() {
    let volume = InMemoryVolume::new("Test").with_space_info(1_000_000, 500_000);
    let space = volume.get_space_info().await.unwrap();
    assert_eq!(
        space,
        SpaceInfo::Bounded {
            total_bytes: 1_000_000,
            available_bytes: 500_000,
            used_bytes: 500_000,
        }
    );
}

#[tokio::test]
async fn storage_with_no_ceiling_reports_what_is_stored_and_nothing_else() {
    // ❗ The shape a quota-less Nextcloud account produces. `available_bytes()`
    // has to answer `None` here, because that is what the copy pre-flight reads
    // as "can't tell, go ahead" and what the pane reads as "draw no bar".
    let volume = InMemoryVolume::new("Test").with_unbounded_space_info(64 * 1024 * 1024);

    let space = volume.get_space_info().await.unwrap();

    assert_eq!(space, SpaceInfo::Unbounded { used_bytes: 67_108_864 });
    assert_eq!(space.used_bytes(), 67_108_864);
    assert_eq!(space.available_bytes(), None);
    assert_eq!(space.total_bytes(), None);
}

// ============================================================================
// scan_for_conflicts tests
// ============================================================================

#[tokio::test]
async fn test_scan_for_conflicts_no_conflicts() {
    let volume = InMemoryVolume::new("Test");
    volume.create_file(app_path!("/existing.txt"), b"data").await.unwrap();

    let source_items = vec![SourceItemInfo {
        name: "other.txt".to_string(),
        size: 100,
        modified: None,
        is_directory: false,
    }];

    let conflicts = volume.scan_for_conflicts(&source_items, app_path!("/")).await.unwrap();
    assert!(conflicts.is_empty());
}

#[tokio::test]
async fn test_scan_for_conflicts_detects_conflict() {
    let volume = InMemoryVolume::new("Test");
    volume
        .create_file(app_path!("/report.txt"), b"old content")
        .await
        .unwrap();

    let source_items = vec![SourceItemInfo {
        name: "report.txt".to_string(),
        size: 200,
        modified: Some(1_700_000_000),
        is_directory: false,
    }];

    let conflicts = volume.scan_for_conflicts(&source_items, app_path!("/")).await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].source_path, "report.txt");
    assert_eq!(conflicts[0].source_size, 200);
    assert_eq!(conflicts[0].dest_size, 11); // "old content"
    // File vs file: both flags false.
    assert!(!conflicts[0].source_is_directory);
    assert!(!conflicts[0].dest_is_directory);
}

#[tokio::test]
async fn test_scan_for_conflicts_populates_directory_flags() {
    let volume = InMemoryVolume::new("Test");
    // Dest holds a directory `photos` and a file `notes.txt`.
    volume.create_directory(app_path!("/photos")).await.unwrap();
    volume.create_file(app_path!("/notes.txt"), b"hello").await.unwrap();

    // Source offers: a dir `photos` (→ dir-vs-dir merge), a file `notes.txt`
    // (→ file-vs-file), and a file `photos.zip` that doesn't clash.
    let source_items = vec![
        SourceItemInfo {
            name: "photos".to_string(),
            size: 0,
            modified: None,
            is_directory: true,
        },
        SourceItemInfo {
            name: "notes.txt".to_string(),
            size: 99,
            modified: None,
            is_directory: false,
        },
        SourceItemInfo {
            name: "photos.zip".to_string(),
            size: 12,
            modified: None,
            is_directory: false,
        },
    ];

    let conflicts = volume.scan_for_conflicts(&source_items, app_path!("/")).await.unwrap();
    assert_eq!(conflicts.len(), 2);

    let dir_conflict = conflicts.iter().find(|c| c.source_path == "photos").unwrap();
    assert!(dir_conflict.source_is_directory, "source dir flag");
    assert!(dir_conflict.dest_is_directory, "dest dir flag");

    let file_conflict = conflicts.iter().find(|c| c.source_path == "notes.txt").unwrap();
    assert!(!file_conflict.source_is_directory);
    assert!(!file_conflict.dest_is_directory);
}

#[tokio::test]
async fn test_scan_for_conflicts_type_mismatch_flags() {
    let volume = InMemoryVolume::new("Test");
    // Dest `data` is a file; source `data` is a directory.
    volume.create_file(app_path!("/data"), b"x").await.unwrap();

    let source_items = vec![SourceItemInfo {
        name: "data".to_string(),
        size: 0,
        modified: None,
        is_directory: true,
    }];

    let conflicts = volume.scan_for_conflicts(&source_items, app_path!("/")).await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].source_is_directory, "source is a dir");
    assert!(!conflicts[0].dest_is_directory, "dest is a file");
}

#[test]
fn test_supports_export() {
    let volume = InMemoryVolume::new("Test");
    assert!(volume.supports_export());
}
