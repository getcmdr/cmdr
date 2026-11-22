//! An explicit transfer leaf (`transfer/DETAILS.md` § Named destinations) is
//! refused at the start, before anything is admitted or written: a leaf that
//! isn't one plain component, a leaf for more than one source, and a leaf too
//! long for the filesystem. Every start that takes a leaf validates it once
//! (the routed copy and move, and the same-`root` move), and no engine below
//! checks it again.

use std::path::PathBuf;
use std::sync::Arc;

use super::event_sinks::{CollectorEventSink, OperationEventSink};
use super::types::{VolumeCopyConfig, WriteOperationConfig, WriteOperationError, WriteOperationStartResult};
use crate::testing::TestOperations;
use crate::types::TransferLeaves;
use cmdr_fs::testing::TestDir;
use cmdr_operation_log::types::Initiator;

/// The starts a leaf can arrive through.
#[derive(Debug, Clone, Copy)]
enum Start {
    /// `start_volume_copy`, as the transfer dialog copies.
    RoutedCopy,
    /// `start_volume_move`, as the transfer dialog moves.
    RoutedMove,
    /// `move_files_start`, the same-`root` move command.
    LocalMove,
}

const STARTS: [Start; 3] = [Start::RoutedCopy, Start::RoutedMove, Start::LocalMove];

/// Starts a copy or move on the local `root` volume through `via`.
async fn start(
    engine: &TestOperations,
    via: Start,
    sources: Vec<PathBuf>,
    destination: &std::path::Path,
    leaf: &str,
) -> Result<WriteOperationStartResult, WriteOperationError> {
    let events: Arc<dyn OperationEventSink> = Arc::new(CollectorEventSink::new());
    let leaves = TransferLeaves::single(leaf);
    let config = VolumeCopyConfig {
        leaves: leaves.clone(),
        ..VolumeCopyConfig::default()
    };
    let dest = destination.display().to_string();
    let (root, initiator) = ("root".to_string(), Initiator::User);
    match via {
        Start::RoutedCopy => {
            engine
                .start_volume_copy(
                    events,
                    root.clone(),
                    crate::testing::app_paths_of(sources),
                    root,
                    dest,
                    config,
                    initiator,
                    None,
                )
                .await
        }
        Start::RoutedMove => {
            engine
                .start_volume_move(
                    events,
                    root.clone(),
                    crate::testing::app_paths_of(sources),
                    root,
                    dest,
                    config,
                    initiator,
                    None,
                )
                .await
        }
        Start::LocalMove => {
            let config = WriteOperationConfig {
                leaves,
                ..WriteOperationConfig::default()
            };
            engine
                .move_files_start(
                    events,
                    crate::testing::app_paths_of(sources),
                    crate::testing::app_path_of(destination),
                    config,
                    vec![],
                    None,
                    initiator,
                    None,
                    None,
                )
                .await
        }
    }
}

fn fixture(tag: &str) -> (TestDir, PathBuf, PathBuf) {
    let tmp = TestDir::new(tag);
    let source = tmp.join("original.txt");
    std::fs::write(&source, b"payload").expect("source");
    let dest = tmp.join("dest");
    std::fs::create_dir(&dest).expect("dest");
    (tmp, source, dest)
}

fn assert_untouched(source: &std::path::Path, dest: &std::path::Path) {
    assert_eq!(std::fs::read(source).expect("the source stays"), b"payload");
    assert_eq!(std::fs::read_dir(dest).expect("dest").count(), 0, "nothing landed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_leaf_that_is_not_one_plain_component_refuses_at_the_start() {
    let engine = TestOperations::new();
    engine.register_local_root();
    for via in STARTS {
        for leaf in ["", ".", "..", "../escape.txt", "/absolute", "bad\0name"] {
            let (_tmp, source, dest) = fixture("named_leaf_entry_bad");
            let result = start(&engine, via, vec![source.clone()], &dest, leaf).await;
            assert!(
                matches!(result, Err(WriteOperationError::InvalidName { .. })),
                "{via:?} leaf={leaf:?}: {result:?}"
            );
            assert_untouched(&source, &dest);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_leaf_for_more_than_one_source_refuses_at_the_start() {
    let engine = TestOperations::new();
    engine.register_local_root();
    for via in STARTS {
        let (tmp, source, dest) = fixture("named_leaf_entry_batch");
        let other = tmp.join("other.txt");
        std::fs::write(&other, b"other").expect("other");
        let result = start(&engine, via, vec![source.clone(), other], &dest, "backup.txt").await;
        assert!(
            matches!(result, Err(WriteOperationError::InvalidName { .. })),
            "{via:?}: {result:?}"
        );
        assert_untouched(&source, &dest);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_leaf_too_long_for_the_filesystem_refuses_at_the_start() {
    let engine = TestOperations::new();
    engine.register_local_root();
    for via in STARTS {
        let (_tmp, source, dest) = fixture("named_leaf_entry_long");
        let result = start(&engine, via, vec![source.clone()], &dest, &"n".repeat(256)).await;
        // Typed as the name being too long, so the dialog says so.
        assert!(
            matches!(result, Err(WriteOperationError::NameTooLong { .. })),
            "{via:?}: {result:?}"
        );
        assert_untouched(&source, &dest);
    }
}
