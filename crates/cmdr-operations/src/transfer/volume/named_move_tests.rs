//! Explicit names share the serial landing and source-sweep paths of both volume moves.
use super::test_support::{make_state, make_volumes};
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use crate::types::ConflictResolution;
use cmdr_fs::app_path;
use std::path::Path;

#[tokio::test]
async fn named_volume_moves_honor_skip_and_overwrite_at_the_requested_leaf() {
    let engine = TestOperations::new();
    for same_volume in [false, true] {
        for policy in [ConflictResolution::Skip, ConflictResolution::Overwrite] {
            let (source, other) = make_volumes();
            let dest = if same_volume { Arc::clone(&source) } else { other };
            source
                .create_file(app_path!("/original.txt"), b"new bytes")
                .await
                .unwrap();
            dest.create_directory(app_path!("/dest")).await.unwrap();
            dest.create_file(app_path!("/dest/renamed.txt"), b"old bytes")
                .await
                .unwrap();
            let config = VolumeCopyConfig {
                leaves: crate::types::TransferLeaves::single("renamed.txt"),
                conflict_resolution: policy,
                ..Default::default()
            };
            let events = Arc::new(CollectorEventSink::new());
            let state = make_state(&engine);
            let sources = [PathBuf::from("/original.txt")];
            if same_volume {
                super::super::move_same::move_within_same_volume_with_progress(
                    events,
                    "named-same-move",
                    &state,
                    Arc::clone(&source),
                    &sources,
                    Path::new("/dest"),
                    &config,
                )
                .await
                .unwrap();
            } else {
                move_volumes_with_progress(
                    events,
                    "named-cross-move",
                    &state,
                    Arc::clone(&source),
                    &crate::testing::app_paths_of(&sources),
                    Arc::clone(&dest),
                    &crate::testing::app_path_of(Path::new("/dest")),
                    &config,
                )
                .await
                .unwrap();
            }
            let mut reader = dest.open_read_stream(app_path!("/dest/renamed.txt")).await.unwrap();
            let skipped = policy == ConflictResolution::Skip;
            let expected = if skipped { b"old bytes" } else { b"new bytes" };
            assert_eq!(reader.next_chunk().await.unwrap().unwrap(), expected);
            assert_eq!(source.exists(app_path!("/original.txt")).await, skipped);
            assert!(!dest.exists(app_path!("/dest/original.txt")).await);
        }
    }
}

#[tokio::test]
async fn named_volume_folder_moves_keep_empty_directories() {
    let engine = TestOperations::new();
    for same_volume in [false, true] {
        let (source, other) = make_volumes();
        let dest = if same_volume { Arc::clone(&source) } else { other };
        source
            .create_directory_all(app_path!("/original/empty/nested"))
            .await
            .unwrap();
        source
            .create_file(app_path!("/original/child.txt"), b"payload")
            .await
            .unwrap();
        let config = VolumeCopyConfig {
            leaves: crate::types::TransferLeaves::single("renamed"),
            ..Default::default()
        };
        let events = Arc::new(CollectorEventSink::new());
        let state = make_state(&engine);
        let sources = [PathBuf::from("/original")];
        if same_volume {
            super::super::move_same::move_within_same_volume_with_progress(
                events,
                "named-same-folder",
                &state,
                Arc::clone(&source),
                &sources,
                Path::new("/dest"),
                &config,
            )
            .await
            .unwrap();
        } else {
            move_volumes_with_progress(
                events,
                "named-cross-folder",
                &state,
                Arc::clone(&source),
                &crate::testing::app_paths_of(&sources),
                Arc::clone(&dest),
                &crate::testing::app_path_of(Path::new("/dest")),
                &config,
            )
            .await
            .unwrap();
        }
        assert!(dest.exists(app_path!("/dest/renamed/child.txt")).await);
        assert!(
            dest.is_directory(app_path!("/dest/renamed/empty/nested"))
                .await
                .unwrap()
        );
        assert!(!source.exists(app_path!("/original")).await);
        assert!(!dest.exists(app_path!("/dest/original")).await);
    }
}
