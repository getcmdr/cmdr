//! A name the destination's rules REFUSE is refused before any write, ❌ never
//! left to the landing: Linux's vfat strips a trailing period with no error, so
//! `dup.` would land on a sibling `dup`, and with an Overwrite policy replace it
//! (`crate::name_rules::DestinationNames`). The destination here is the scratch
//! disk with a Linux host's FAT rules preset, since tests may not mount a FAT
//! image.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::validation::{NameRules, NameTier};

use super::single_item::copy_single_item;
use crate::conflict::ApplyToAll;
use crate::ledger::CopyTransaction;
use crate::scan_source_tracker::FileVerdict;
use crate::state::WriteOperationState;
use crate::testing::TestOperations;
use crate::transfer::conflict_responder_test_support::ConflictResponderSink;
use crate::transfer_tree::TreeLeaf;
use crate::types::{
    ConflictResolution, RefusedNameReason, WriteOperationConfig, WriteOperationError, WriteOperationType,
};

/// A Linux host's vfat stick: the characters and a trailing period refused.
fn fat_on_linux() -> NameRules {
    NameRules {
        windows_characters: NameTier::Refuse,
        reserved_names: NameTier::Warn,
        ..NameRules::windows_format()
    }
}

struct Copied {
    result: Result<FileVerdict, WriteOperationError>,
    transaction: CopyTransaction,
    prompts: Vec<crate::types::WriteConflictEvent>,
}

/// Copies `source` to `dest_root/relative` with "Overwrite all" remembered, the
/// destination refusing what `fat_on_linux` refuses, answering a prompt with `answer`.
fn copy_under_fat_rules(source: &Path, dest_root: &Path, relative: &str, answer: ConflictResolution) -> Copied {
    let engine = TestOperations::new();
    let state: Arc<WriteOperationState> = Arc::new(engine.new_operation_state(Duration::from_millis(200)));
    state.destination_names.resolve(fat_on_linux);
    let events = ConflictResponderSink::new(&state, answer, false);
    let config = WriteOperationConfig {
        conflict_resolution: ConflictResolution::Overwrite,
        ..WriteOperationConfig::default()
    };
    let mut transaction = CopyTransaction::new();
    let (mut files_done, mut bytes_done) = (0usize, 0u64);
    let mut skipped_subtrees = HashSet::new();
    let result = copy_single_item(
        source,
        dest_root.join(relative),
        dest_root,
        None,
        false,
        4,
        &mut files_done,
        &mut bytes_done,
        1,
        4,
        &state,
        &events,
        "op-refused-class",
        WriteOperationType::Copy,
        &Duration::from_millis(200),
        &config,
        &mut transaction,
        &mut ApplyToAll::default(),
        &mut HashSet::new(),
        &mut HashMap::new(),
        &mut skipped_subtrees,
        &mut HashSet::new(),
        None,
        &TreeLeaf::detached(),
    );
    let prompts = events.inner.conflicts.lock().unwrap().clone();
    Copied {
        result,
        transaction,
        prompts,
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_trailing_period_is_asked_about_and_never_overwrites_its_sibling() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("dup.");
    fs::write(&source, b"incoming").unwrap();
    let dest = tmp.path().join("dst");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("dup"), b"the user's dup").unwrap();

    let copied = copy_under_fat_rules(&source, &dest, "dup.", ConflictResolution::Skip);

    assert!(matches!(copied.result, Ok(FileVerdict::Skipped)), "{:?}", copied.result);
    assert_eq!(copied.prompts.len(), 1, "one refused-name prompt: {:?}", copied.prompts);
    assert_eq!(copied.prompts[0].refused_name, Some(RefusedNameReason::NotAllowed));
    assert_eq!(fs::read(dest.join("dup")).unwrap(), b"the user's dup");
    assert_eq!(names_in(&dest), ["dup"], "nothing landed");
    assert!(copied.transaction.created_files().is_empty());
    copied.transaction.commit();
}

#[test]
fn a_stop_ends_the_copy_with_the_refusal() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("a:b");
    fs::write(&source, b"incoming").unwrap();
    let dest = tmp.path().join("dst");
    fs::create_dir_all(&dest).unwrap();

    let copied = copy_under_fat_rules(&source, &dest, "a:b", ConflictResolution::Rename);

    assert!(
        matches!(copied.result, Err(WriteOperationError::InvalidName { .. })),
        "{:?}",
        copied.result
    );
    assert!(names_in(&dest).is_empty());
    copied.transaction.commit();
}

/// A FOLDER whose name the destination refuses would merge into the sibling the
/// driver strips it onto. Asked once, as a folder; a Skip leaves it out whole.
#[test]
fn a_refused_folder_is_asked_about_as_a_folder_and_nothing_under_it_lands() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("dir.");
    fs::create_dir_all(&folder).unwrap();
    let source = folder.join("a.txt");
    fs::write(&source, b"incoming").unwrap();
    let dest = tmp.path().join("dst");
    fs::create_dir_all(dest.join("dir")).unwrap();
    fs::write(dest.join("dir").join("a.txt"), b"the user's a").unwrap();

    let copied = copy_under_fat_rules(&source, &dest, "dir./a.txt", ConflictResolution::Skip);

    assert!(matches!(copied.result, Ok(FileVerdict::Skipped)), "{:?}", copied.result);
    assert_eq!(copied.prompts.len(), 1, "{:?}", copied.prompts);
    assert!(copied.prompts[0].source_is_directory);
    assert_eq!(fs::read(dest.join("dir").join("a.txt")).unwrap(), b"the user's a");
    assert_eq!(names_in(&dest), ["dir"]);
    copied.transaction.commit();
}

/// Rules that only warn (a Mac's drivers store a stand-in) refuse nothing.
#[test]
fn a_warned_name_still_lands() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("plain.txt");
    fs::write(&source, b"incoming").unwrap();
    let dest = tmp.path().join("dst");
    fs::create_dir_all(&dest).unwrap();
    let dest_path: PathBuf = dest.join("plain.txt");

    let copied = copy_under_fat_rules(&source, &dest, "plain.txt", ConflictResolution::Skip);

    assert!(
        matches!(copied.result, Ok(FileVerdict::CarriedOut)),
        "{:?}",
        copied.result
    );
    assert!(copied.prompts.is_empty());
    assert!(dest_path.exists());
    copied.transaction.commit();
}
