//! A clash answered with "… all" changes the policy the operation reports
//! (`conflict_policy_cell.rs`), on the volume engine. The local engine's twin is
//! `transfer/conflict_policy_latch_tests.rs`.

use super::*;
use crate::testing::TestOperations;
use crate::transfer::conflict_responder_test_support::ConflictResponderSink;
use cmdr_fs::app_path;
use std::path::Path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skip_all_answer_becomes_the_reported_policy() {
    let engine = TestOperations::new();
    let (source, dest) = make_volumes();
    source.create_file(app_path!("/a.txt"), b"new a").await.unwrap();
    source.create_file(app_path!("/b.txt"), b"new b").await.unwrap();
    dest.create_file(app_path!("/a.txt"), b"old a").await.unwrap();
    dest.create_file(app_path!("/b.txt"), b"old b").await.unwrap();

    let state = Arc::new(
        engine
            .new_operation_state(Duration::from_millis(50))
            .with_conflict_policy(ConflictResolution::Stop),
    );
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, true));
    let config = VolumeCopyConfig {
        conflict_resolution: ConflictResolution::Stop,
        ..VolumeCopyConfig::default()
    };

    let result = copy_volumes_with_progress(
        events.clone(),
        "test-op-policy-latch",
        &state,
        Arc::clone(&source),
        &crate::testing::app_paths_of(&[PathBuf::from("/a.txt"), PathBuf::from("/b.txt")]),
        Arc::clone(&dest),
        &crate::testing::app_path_of(Path::new("/")),
        &config,
    )
    .await;

    assert!(result.is_ok(), "copy should succeed: {result:?}");
    assert_eq!(state.conflict_policy.current(), Some(ConflictResolution::Skip));
}
