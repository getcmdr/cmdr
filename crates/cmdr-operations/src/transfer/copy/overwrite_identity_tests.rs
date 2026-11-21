//! An Overwrite onto an existing file, and the per-file "is it the same file?" guard
//! in front of it (`single_item.rs`). Runs on every OS: on Windows the guard reads
//! the node by path (`cmdr_fs::file_identity::same_node_at`), since no `Metadata`
//! carries an id there (#1291).

use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use cmdr_fs::testing::TestDir;

fn temp(name: &str) -> TestDir {
    TestDir::new(&format!("copy_overwrite_identity_{}_{}", name, uuid::Uuid::new_v4()))
}

fn overwrite() -> WriteOperationConfig {
    WriteOperationConfig {
        conflict_resolution: ConflictResolution::Overwrite,
        ..Default::default()
    }
}

/// `src/<name>/notes.txt` holding `NEW` and `dst/<name>/notes.txt` holding `OLD`:
/// copying the folder merges, and the one file inside clashes. Answers the source
/// folder and the two files.
fn plant_clash(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let incoming = root.join("src").join("docs");
    let landing = root.join("dst").join("docs");
    fs::create_dir_all(&incoming).unwrap();
    fs::create_dir_all(&landing).unwrap();
    fs::write(incoming.join("notes.txt"), "NEW").unwrap();
    fs::write(landing.join("notes.txt"), "OLD").unwrap();
    (incoming.clone(), incoming.join("notes.txt"), landing.join("notes.txt"))
}

#[test]
fn an_overwrite_onto_an_existing_file_lands_the_new_bytes() {
    let engine = TestOperations::new();
    let dir = temp("lands");
    let (source, _, existing) = plant_clash(&dir);
    let events = CollectorEventSink::new();

    copy_files_with_progress_inner(
        &events,
        "op-overwrite-lands",
        &Arc::new(engine.new_operation_state(Duration::from_millis(0))),
        std::slice::from_ref(&source),
        &dir.join("dst"),
        &overwrite(),
    )
    .expect("the overwrite copy succeeds");

    assert_eq!(
        fs::read_to_string(&existing).unwrap(),
        "NEW",
        "the person's Overwrite happened"
    );
}

/// ❗ Can't tell whether the two are one file: an overwrite might truncate its own
/// source, so it refuses, and says so. ❌ Never a silent skip that reports success.
#[test]
fn an_overwrite_that_cant_tell_whether_the_two_are_one_file_refuses() {
    let engine = TestOperations::new();
    let dir = temp("unknown");
    let (source, incoming, existing) = plant_clash(&dir);
    let _unknown = cmdr_fs::testing::hide_identity_of(&[&incoming, &existing]);

    let result = copy_files_with_progress_inner(
        &CollectorEventSink::new(),
        "op-overwrite-unknown",
        &Arc::new(engine.new_operation_state(Duration::from_millis(0))),
        std::slice::from_ref(&source),
        &dir.join("dst"),
        &overwrite(),
    );

    assert!(
        matches!(result, Err(WriteOperationError::CantTellIfSameFile { ref path }) if *path == existing.display().to_string()),
        "got {result:?}"
    );
    assert_eq!(fs::read_to_string(&existing).unwrap(), "OLD", "nothing was overwritten");
    assert_eq!(fs::read_to_string(&incoming).unwrap(), "NEW", "the source is untouched");
}
