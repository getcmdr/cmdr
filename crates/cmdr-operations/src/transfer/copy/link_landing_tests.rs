//! A copied LINK meeting an existing entry on a destination that can't hold links
//! (exFAT, FAT, some SMB shares, Windows without the privilege).
//!
//! The link is made under a `.cmdr-tmp-*` sibling and renamed over the
//! destination, so a refusal to make it leaves the person's file exactly as it
//! was. These cells arm the scope's `refuse_links` seam, the refusal those
//! drives give, and check the bytes that were there.

use super::super::conflict_responder_test_support::ConflictResponderSink;
use super::*;
use crate::event_sinks::CollectorEventSink;
use crate::testing::TestOperations;
use cmdr_fs::testing::TestDir;

const USER_BYTES: &str = "THE PERSON'S OWN FILE";

fn temp(name: &str) -> TestDir {
    TestDir::new(&format!("copy_link_landing_{}_{}", name, uuid::Uuid::new_v4()))
}

fn refusing_state(engine: &TestOperations) -> Arc<WriteOperationState> {
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(0)));
    state.scope().refuse_links();
    state
}

/// `src/note.txt` is a link; answers the source.
fn plant_source_link(root: &Path) -> PathBuf {
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(root.join("target.txt"), "TARGET").unwrap();
    let link = src.join("note.txt");
    cmdr_fs::testing::symlink(root.join("target.txt"), &link).unwrap();
    link
}

/// Every name in `dir`, sorted, so a stray temp shows up.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn an_overwrite_with_a_link_the_destination_refuses_keeps_the_file_that_was_there() {
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    let engine = TestOperations::new();
    let dir = temp("file");
    let link = plant_source_link(&dir);
    let dst = dir.join("dst");
    fs::create_dir_all(&dst).unwrap();
    fs::write(dst.join("note.txt"), USER_BYTES).unwrap();

    let result = copy_files_with_progress_inner(
        &CollectorEventSink::new(),
        "op-link-refused-over-a-file",
        &refusing_state(&engine),
        std::slice::from_ref(&link),
        &dst,
        &WriteOperationConfig {
            conflict_resolution: ConflictResolution::Overwrite,
            ..Default::default()
        },
    );

    let kept = dst.join("note.txt");
    assert!(!kept.is_symlink(), "the file stays a file");
    assert_eq!(
        fs::read_to_string(&kept).unwrap(),
        USER_BYTES,
        "the person's bytes survive"
    );
    assert_eq!(names_in(&dst), ["note.txt"], "no temp is left beside it");
    assert!(
        matches!(&result, Err(WriteOperationError::LinksNotSupported { path }) if *path == dst.join("note.txt").display().to_string()),
        "expected LinksNotSupported naming the destination, got {result:?}"
    );
}

#[test]
fn a_new_link_the_destination_refuses_leaves_nothing_behind() {
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    let engine = TestOperations::new();
    let dir = temp("fresh");
    let link = plant_source_link(&dir);
    let dst = dir.join("dst");
    fs::create_dir_all(&dst).unwrap();

    let result = copy_files_with_progress_inner(
        &CollectorEventSink::new(),
        "op-link-refused-fresh",
        &refusing_state(&engine),
        std::slice::from_ref(&link),
        &dst,
        &WriteOperationConfig::default(),
    );

    assert!(
        matches!(result, Err(WriteOperationError::LinksNotSupported { .. })),
        "got {result:?}"
    );
    assert!(names_in(&dst).is_empty(), "nothing lands");
}

/// A link meeting a FOLDER, Overwrite answered: the folder comes back whole.
#[test]
fn an_overwrite_of_a_folder_with_a_refused_link_puts_the_folder_back() {
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    let engine = TestOperations::new();
    let dir = temp("folder");
    let link = plant_source_link(&dir);
    let dst = dir.join("dst");
    fs::create_dir_all(dst.join("note.txt")).unwrap();
    fs::write(dst.join("note.txt/inside.txt"), USER_BYTES).unwrap();
    let state = refusing_state(&engine);
    let events = ConflictResponderSink::new(&state, ConflictResolution::Overwrite, false);

    let result = copy_files_with_progress_inner(
        &events,
        "op-link-refused-over-a-folder",
        &state,
        std::slice::from_ref(&link),
        &dst,
        &WriteOperationConfig {
            conflict_resolution: ConflictResolution::Stop,
            ..Default::default()
        },
    );

    assert!(
        matches!(result, Err(WriteOperationError::LinksNotSupported { .. })),
        "got {result:?}"
    );
    assert_eq!(
        fs::read_to_string(dst.join("note.txt/inside.txt")).unwrap(),
        USER_BYTES,
        "the folder and its file survive"
    );
    assert_eq!(names_in(&dst), ["note.txt"], "no aside is left beside it");
}

/// The same Overwrite on a drive that holds links: the link replaces the file,
/// and the staging temp is gone.
#[test]
fn an_overwrite_with_a_link_lands_the_link_and_leaves_no_temp() {
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    let engine = TestOperations::new();
    let dir = temp("lands");
    let link = plant_source_link(&dir);
    let dst = dir.join("dst");
    fs::create_dir_all(&dst).unwrap();
    fs::write(dst.join("note.txt"), USER_BYTES).unwrap();

    let result = copy_files_with_progress_inner(
        &CollectorEventSink::new(),
        "op-link-lands-over-a-file",
        &Arc::new(engine.new_operation_state(Duration::from_millis(0))),
        std::slice::from_ref(&link),
        &dst,
        &WriteOperationConfig {
            conflict_resolution: ConflictResolution::Overwrite,
            ..Default::default()
        },
    );

    assert!(result.is_ok(), "got {result:?}");
    let landed = dst.join("note.txt");
    assert!(landed.is_symlink(), "the link replaced the file");
    assert_eq!(fs::read_link(&landed).unwrap(), dir.join("target.txt"));
    assert_eq!(names_in(&dst), ["note.txt"], "no temp is left beside it");
}
