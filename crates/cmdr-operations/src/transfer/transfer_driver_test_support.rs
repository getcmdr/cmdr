//! Shared fixtures for the `transfer_driver.rs` test suites
//! (`transfer_driver_pre_skip_tests.rs`, `transfer_driver_sync_tests.rs`,
//! `transfer_driver_async_tests.rs`, `transfer_driver_concurrent_tests.rs`).
//!
//! Items are `pub(super)` so the sibling test modules (all children of the
//! `transfer_driver` module) can reach them through `super::test_support::…`.

use super::super::super::state::WriteOperationState;
use super::super::super::types::{ConflictResolution, TransferLeaves, WriteOperationPhase, WriteOperationType};
use super::DriverConfig;
use crate::testing::TestOperations;
use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::ignore_poison::IgnorePoison;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(super) fn make_state(engine: &TestOperations) -> Arc<WriteOperationState> {
    // Zero progress interval so throttled emits ALWAYS fire — tests that
    // count emits would otherwise be flaky.
    Arc::new(engine.new_operation_state(Duration::from_millis(0)))
}

pub(super) fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

pub(super) fn copy_config() -> DriverConfig {
    DriverConfig {
        operation_type: WriteOperationType::Copy,
        phase: WriteOperationPhase::Copying,
        conflict_resolution: ConflictResolution::Stop,
        pre_known_conflicts: Vec::new(),
        // Driver unit tests use bare closures that emit no progress of their
        // own, so they exercise the driver's per-source milestone path.
        emit_per_source_milestone: true,
        skip_ledger: None,
        report_source_outcomes: false,
        leaves: TransferLeaves::default(),
    }
}

/// Tiny in-memory "call log" the closures dump into so tests can assert
/// invocation order and counts.
#[derive(Default)]
pub(super) struct CallLog {
    invoked_for: Mutex<Vec<PathBuf>>,
    invoked_dests: Mutex<Vec<Option<PathBuf>>>,
}

impl CallLog {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    pub(super) fn record<P: TestPath + ?Sized>(&self, src: &P, dest: Option<&P>) {
        self.invoked_for.lock_ignore_poison().push(src.test_path());
        self.invoked_dests
            .lock_ignore_poison()
            .push(dest.map(TestPath::test_path));
    }
    pub(super) fn sources(&self) -> Vec<PathBuf> {
        self.invoked_for.lock_ignore_poison().clone()
    }
    pub(super) fn dests(&self) -> Vec<Option<PathBuf>> {
        self.invoked_dests.lock_ignore_poison().clone()
    }
}

/// A path a driver hands its closures, as the call log records it: the sync
/// driver's `std::path` values, and the async driver's app paths (test paths are
/// POSIX text, so their app text is their `std::path` text).
pub(super) trait TestPath {
    fn test_path(&self) -> PathBuf;
}

impl TestPath for Path {
    fn test_path(&self) -> PathBuf {
        self.to_path_buf()
    }
}

impl TestPath for AppPath {
    fn test_path(&self) -> PathBuf {
        crate::testing::engine_path_of(self)
    }
}

/// A test's pre-skip set as the async driver takes it.
pub(super) fn typed_set(set: &HashSet<PathBuf>) -> HashSet<AppPathBuf> {
    set.iter().map(|path| crate::testing::app_path_of(path)).collect()
}
