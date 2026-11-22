//! The vocabulary every write operation shares: the cancel intent machine, the
//! config the frontend sends, and how an `io::Error` becomes a typed
//! `WriteOperationError`.
//!
//! Everything with a subject of its own lives next to that subject:
//! `overwrite_tests.rs`, `ledger_tests.rs` (`CopyTransaction`), and
//! `delete/delete_cancel_tests.rs`.
//!
//! Deserialization is tested and serialization isn't: the derive macros are
//! well covered upstream, and it's the DEserialization that pins the API
//! contract with the frontend.

use super::*;
use crate::testing::TestOperations;
use cmdr_fs::testing::TestDir;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Creates a temporary test directory with a unique name.
fn create_temp_dir(name: &str) -> TestDir {
    TestDir::new(&format!("write_test_{}", name))
}

// ============================================================================
// Cancellation state tests
// ============================================================================

#[test]
fn test_cancel_sets_intent() {
    let state = Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(200)));

    assert!(!state.intent().is_cancelled());
    assert_eq!(state.intent().load(), OperationIntent::Running);

    state.intent().set_for_test(OperationIntent::Stopped);
    assert!(state.intent().is_cancelled());
    assert_eq!(state.intent().load(), OperationIntent::Stopped);
}

// ============================================================================
// Config tests - deserialization verifies API contract with frontend
// ============================================================================

#[test]
fn test_default_config() {
    let config = WriteOperationConfig::default();
    assert_eq!(config.progress_interval_ms, 200);
    assert_eq!(config.conflict_resolution, ConflictResolution::Stop);
    assert!(!config.dry_run);
}

#[test]
fn test_config_default_values_deserialization() {
    let json = r#"{}"#;
    let config: WriteOperationConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.progress_interval_ms, 200);
    assert!(!config.dry_run);
}

#[test]
fn test_config_conflict_resolution_deserialization() {
    let json = r#"{"conflictResolution": "skip"}"#;
    let config: WriteOperationConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.conflict_resolution, ConflictResolution::Skip);

    let json = r#"{"conflictResolution": "overwrite"}"#;
    let config: WriteOperationConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.conflict_resolution, ConflictResolution::Overwrite);

    let json = r#"{"conflictResolution": "rename"}"#;
    let config: WriteOperationConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.conflict_resolution, ConflictResolution::Rename);
}

#[test]
fn test_config_dry_run_deserialization() {
    let json = r#"{"dryRun": true}"#;
    let config: WriteOperationConfig = serde_json::from_str(json).unwrap();
    assert!(config.dry_run);
}

// ============================================================================
// IO Error conversion tests - actual logic
// ============================================================================

#[test]
fn test_io_error_not_found_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let write_err: WriteOperationError = io_err.into();

    assert!(matches!(write_err, WriteOperationError::SourceNotFound { .. }));
}

#[test]
fn test_io_error_permission_denied_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
    let write_err: WriteOperationError = io_err.into();

    assert!(matches!(write_err, WriteOperationError::PermissionDenied { .. }));
}

#[test]
fn test_io_error_already_exists_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::AlreadyExists, "file exists");
    let write_err: WriteOperationError = io_err.into();

    assert!(matches!(write_err, WriteOperationError::DestinationExists { .. }));
}

#[test]
fn test_io_error_other_conversion() {
    let io_err = std::io::Error::other("some error");
    let write_err: WriteOperationError = io_err.into();

    assert!(matches!(write_err, WriteOperationError::IoError { .. }));
}

// ============================================================================
// Temp directory helper tests
// ============================================================================

#[test]
fn test_create_and_cleanup_temp_dir() {
    // The subject here is the fixture lifecycle itself, so the drop has to be
    // explicit: cleanup is no longer a call the test makes, it's `TestDir`'s
    // `Drop`. Capture the path first, since the handle is what owns it.
    let temp_dir = create_temp_dir("helper_test");
    let path = temp_dir.to_path_buf();
    assert!(temp_dir.exists());
    assert!(temp_dir.is_dir());

    drop(temp_dir);
    assert!(!path.exists(), "dropping the handle must remove the directory");
}

// ============================================================================
// OperationIntent transitions
// ============================================================================

#[test]
fn test_operation_intent_transitions() {
    let state = Arc::new(TestOperations::new().new_operation_state(Duration::from_millis(200)));

    // Running → RollingBack
    assert_eq!(state.intent().load(), OperationIntent::Running);
    state.intent().set_for_test(OperationIntent::RollingBack);
    assert_eq!(state.intent().load(), OperationIntent::RollingBack);
    assert!(state.intent().is_cancelled(), "RollingBack should count as cancelled");

    // RollingBack → Stopped
    state.intent().set_for_test(OperationIntent::Stopped);
    assert_eq!(state.intent().load(), OperationIntent::Stopped);
    assert!(state.intent().is_cancelled(), "Stopped should count as cancelled");

    // Running → Stopped (direct, no rollback)
    state.intent().set_for_test(OperationIntent::Running);
    state.intent().set_for_test(OperationIntent::Stopped);
    assert_eq!(state.intent().load(), OperationIntent::Stopped);
}

// ============================================================================
// Whole-crate guard: no global sync anywhere in the engine
// ============================================================================

/// Delete and trash deliberately don't flush after deleting, and copy/move
/// flush their own destinations (`durability::flush_created_destinations`) rather
/// than firing a whole-machine `libc::sync()`. Clippy refuses `libc::sync`
/// in this crate (`clippy.toml`); this pins the other half, that the deleted
/// `spawn_async_sync` helper has no caller left, so the global-sync approach
/// can't creep back in. (A non-durable delete fails annoyance-class — a deleted
/// file reappears after a crash, the user re-deletes; never data loss — so
/// targeted fsync isn't worth its cost there.)
#[test]
fn no_spawn_async_sync_in_the_engine() {
    fn walk(dir: &Path, skip: &Path, hits: &mut Vec<String>, scanned: &mut usize) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, skip, hits, scanned);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // Skip this very file: it names the forbidden symbol in prose.
            if path == skip {
                continue;
            }
            let src = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            *scanned += 1;
            if src.contains("spawn_async_sync") {
                hits.push(path.display().to_string());
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // Fail closed: a moved or renamed root would otherwise scan nothing and pass.
    assert!(root.is_dir(), "the scan root {} must exist", root.display());
    let mut hits = Vec::new();
    let mut scanned = 0;
    walk(&root, &root.join("tests.rs"), &mut hits, &mut scanned);
    assert!(
        scanned > 300,
        // allowed-pluralize-noun: the assert above it only fires past 300, so the count is never one.
        "scanned only {scanned} files under {}; the engine has hundreds",
        root.display()
    );
    assert!(
        hits.is_empty(),
        "found references to the deleted global-sync helper (spawn_async_sync) in: {hits:?}"
    );
}

/// `cmdr_fs::testing`'s load-time hook reaches a CONSUMER's test binary: a bare
/// `tempfile::tempdir()` here is spelled long on Windows, where CI's `TEMP` is
/// `C:\Users\RUNNER~1\…`. Elsewhere there are no short names, and this holds trivially.
#[test]
fn a_bare_tempdir_in_this_crates_tests_is_spelled_long() {
    let bare = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        cmdr_fs::testing::long_path(bare.path()).expect("long name"),
        bare.path()
    );
}
