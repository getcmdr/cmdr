//! A copy onto a destination that holds the name in another Unicode spelling.
//!
//! `InMemoryVolume` matches names byte-for-byte, the way an SMB share, SFTP, or
//! a phone does, so `café` composed and `café` decomposed are two entries to it:
//! exactly the destination where a copy asking in the wrong spelling would stand
//! a second, identical-looking entry beside the user's. Every cell ends by
//! counting what the folder holds. The same guard against a real server:
//! `operations_host/backend_suites/smb_look_alike_test.rs`. A case-only difference is a
//! separate name and stays the backend's call (`merge_case_fold_tests.rs`).

use super::super::super::conflict_responder_test_support::{ConflictResponderSink, file_conflict_count};
use super::tests::make_state;
use super::*;
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, VolumeCopyConfig, WriteOperationError};
use cmdr_fs::volume::InMemoryVolume;

const CAFE_NFC: &str = "caf\u{e9}.txt";
const CAFE_NFD: &str = "cafe\u{301}.txt";
const FOTOK_NFC: &str = "fot\u{f3}k";
const FOTOK_NFD: &str = "foto\u{301}k";

fn volume(name: &str) -> Arc<InMemoryVolume> {
    Arc::new(InMemoryVolume::new(name).with_space_info(10_000_000, 10_000_000))
}

async fn put(volume: &InMemoryVolume, path: &str, content: &[u8]) {
    volume
        .create_file(AppPath::from_normalized(path).expect("a canonical path"), content)
        .await
        .unwrap();
}

async fn mkdir(volume: &InMemoryVolume, path: &str) {
    volume
        .create_directory(AppPath::from_normalized(path).expect("a canonical path"))
        .await
        .unwrap();
}

/// The names `dir` holds, byte for byte, sorted.
async fn names_in(volume: &InMemoryVolume, dir: &str) -> Vec<String> {
    let mut names: Vec<String> = volume
        .list_directory(AppPath::from_normalized(dir).expect("a canonical path"), None)
        .await
        .unwrap_or_else(|e| panic!("list {dir}: {e:?}"))
        .into_iter()
        .map(|e| e.name)
        .collect();
    names.sort();
    names
}

async fn bytes_at(volume: &InMemoryVolume, path: &str) -> Vec<u8> {
    let mut stream = volume
        .open_read_stream(AppPath::from_normalized(path).expect("a canonical path"))
        .await
        .unwrap_or_else(|e| panic!("open {path}: {e:?}"));
    let mut out = Vec::new();
    while let Some(Ok(chunk)) = stream.next_chunk().await {
        out.extend_from_slice(&chunk);
    }
    out
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    names.sort();
    names
}

/// Copies `sources` into `dest_dir` under `policy`, answering any prompt with
/// Skip, and hands back the outcome, the prompts, and the skip counters.
async fn copy(
    engine: &TestOperations,
    source: &Arc<InMemoryVolume>,
    sources: &[&str],
    dest: &Arc<InMemoryVolume>,
    dest_dir: &str,
    policy: ConflictResolution,
) -> (
    Result<(), WriteFailure>,
    Arc<ConflictResponderSink>,
    Arc<WriteOperationState>,
) {
    let state = make_state(engine);
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, false));
    let config = VolumeCopyConfig {
        conflict_resolution: policy,
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };
    let paths: Vec<PathBuf> = sources.iter().map(PathBuf::from).collect();
    let outcome = copy_volumes_with_progress(
        events.clone(),
        "op-look-alike",
        &state,
        Arc::clone(source) as Arc<dyn Volume>,
        &paths,
        Arc::clone(dest) as Arc<dyn Volume>,
        Path::new(dest_dir),
        &config,
    )
    .await;
    (outcome, events, state)
}

/// Source `/album/café.txt` (NFD) onto a destination `/album` holding the NFC
/// spelling.
async fn album_pair() -> (Arc<InMemoryVolume>, Arc<InMemoryVolume>) {
    let source = volume("Source");
    mkdir(&source, "/album").await;
    put(&source, &format!("/album/{CAFE_NFD}"), b"SOURCE").await;
    let dest = volume("Dest");
    mkdir(&dest, "/album").await;
    put(&dest, &format!("/album/{CAFE_NFC}"), b"THE USER'S FILE").await;
    (source, dest)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deep_look_alike_under_skip_leaves_the_users_file_alone() {
    let engine = TestOperations::new();
    let (source, dest) = album_pair().await;

    let (outcome, _events, state) = copy(&engine, &source, &["/album"], &dest, "/", ConflictResolution::Skip).await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(
        names_in(&dest, "/album").await,
        sorted(&[CAFE_NFC]),
        "no second spelling"
    );
    assert_eq!(bytes_at(&dest, &format!("/album/{CAFE_NFC}")).await, b"THE USER'S FILE");
    assert_eq!(
        state.skipped_totals().0,
        1,
        "the look-alike is reported as the skip it is"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deep_look_alike_under_overwrite_is_replaced_in_place() {
    let engine = TestOperations::new();
    let (source, dest) = album_pair().await;

    let (outcome, _events, _state) =
        copy(&engine, &source, &["/album"], &dest, "/", ConflictResolution::Overwrite).await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(
        names_in(&dest, "/album").await,
        sorted(&[CAFE_NFC]),
        "the overwrite lands on the entry that's there, in its own spelling"
    );
    assert_eq!(bytes_at(&dest, &format!("/album/{CAFE_NFC}")).await, b"SOURCE");
}

/// Under Stop the person is asked, about the entry that's THERE.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_look_alike_prompts_about_the_stored_entry() {
    let engine = TestOperations::new();
    let (source, dest) = album_pair().await;

    let (outcome, events, _state) = copy(&engine, &source, &["/album"], &dest, "/", ConflictResolution::Stop).await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(file_conflict_count(&events.inner), 1, "one prompt, like any clash");
    let conflicts = events.inner.conflicts.lock_ignore_poison();
    assert_eq!(conflicts[0].destination_path, format!("/album/{CAFE_NFC}"));
    assert!(
        conflicts[0].destination_is_look_alike,
        "the prompt says the two names only look the same"
    );
}

/// The top-level prompt (the serial driver's pre-check) says so too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_top_level_look_alike_prompt_says_it_is_one() {
    let engine = TestOperations::new();
    let source = volume("Source");
    put(&source, &format!("/{CAFE_NFD}"), b"SOURCE").await;
    let dest = volume("Dest");
    mkdir(&dest, "/dest").await;
    put(&dest, &format!("/dest/{CAFE_NFC}"), b"THE USER'S FILE").await;

    let (outcome, events, _state) = copy(
        &engine,
        &source,
        &[&format!("/{CAFE_NFD}")],
        &dest,
        "/dest",
        ConflictResolution::Stop,
    )
    .await;

    assert!(outcome.is_ok(), "{outcome:?}");
    let conflicts = events.inner.conflicts.lock_ignore_poison();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].destination_path, format!("/dest/{CAFE_NFC}"));
    assert!(conflicts[0].destination_is_look_alike);
}

/// A name spelled the same way on both sides is an ordinary clash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_exact_clash_is_not_called_a_look_alike() {
    let engine = TestOperations::new();
    let source = volume("Source");
    put(&source, &format!("/{CAFE_NFC}"), b"SOURCE").await;
    let dest = volume("Dest");
    mkdir(&dest, "/dest").await;
    put(&dest, &format!("/dest/{CAFE_NFC}"), b"THE USER'S FILE").await;

    let (outcome, events, _state) = copy(
        &engine,
        &source,
        &[&format!("/{CAFE_NFC}")],
        &dest,
        "/dest",
        ConflictResolution::Stop,
    )
    .await;

    assert!(outcome.is_ok(), "{outcome:?}");
    let conflicts = events.inner.conflicts.lock_ignore_poison();
    assert_eq!(conflicts.len(), 1);
    assert!(!conflicts[0].destination_is_look_alike);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_keeps_the_users_file_and_adds_a_numbered_one() {
    let engine = TestOperations::new();
    let (source, dest) = album_pair().await;

    let (outcome, _events, _state) = copy(&engine, &source, &["/album"], &dest, "/", ConflictResolution::Rename).await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(
        names_in(&dest, "/album").await,
        sorted(&[CAFE_NFC, "caf\u{e9} (1).txt"])
    );
    assert_eq!(bytes_at(&dest, &format!("/album/{CAFE_NFC}")).await, b"THE USER'S FILE");
}

/// A folder spelled another way merges into the folder that's there, with no
/// prompt, exactly as an exact name would.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_look_alike_folder_merges_into_the_one_that_is_there() {
    let engine = TestOperations::new();
    let source = volume("Source");
    mkdir(&source, "/album").await;
    mkdir(&source, &format!("/album/{FOTOK_NFD}")).await;
    put(&source, &format!("/album/{FOTOK_NFD}/new.txt"), b"SOURCE").await;
    let dest = volume("Dest");
    mkdir(&dest, "/album").await;
    mkdir(&dest, &format!("/album/{FOTOK_NFC}")).await;
    put(&dest, &format!("/album/{FOTOK_NFC}/keep.txt"), b"THE USER'S FILE").await;

    let (outcome, events, _state) = copy(&engine, &source, &["/album"], &dest, "/", ConflictResolution::Stop).await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(names_in(&dest, "/album").await, sorted(&[FOTOK_NFC]));
    assert_eq!(
        names_in(&dest, &format!("/album/{FOTOK_NFC}")).await,
        sorted(&["keep.txt", "new.txt"])
    );
    assert_eq!(file_conflict_count(&events.inner), 0, "a folder merge never prompts");
}

/// A single top-level source takes the SERIAL driver, whose pre-check is a
/// per-source probe rather than the level listing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_top_level_look_alike_is_a_conflict_on_the_serial_driver() {
    let engine = TestOperations::new();
    let source = volume("Source");
    put(&source, &format!("/{CAFE_NFD}"), b"SOURCE").await;
    let dest = volume("Dest");
    mkdir(&dest, "/dest").await;
    put(&dest, &format!("/dest/{CAFE_NFC}"), b"THE USER'S FILE").await;

    let (outcome, _events, state) = copy(
        &engine,
        &source,
        &[&format!("/{CAFE_NFD}")],
        &dest,
        "/dest",
        ConflictResolution::Skip,
    )
    .await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(names_in(&dest, "/dest").await, sorted(&[CAFE_NFC]));
    assert_eq!(state.skipped_totals().0, 1);
}

/// Three or more sources take the CONCURRENT driver.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_top_level_look_alike_is_overwritten_in_place_on_the_concurrent_driver() {
    let engine = TestOperations::new();
    let source = volume("Source");
    for name in ["a.txt", "b.txt", CAFE_NFD] {
        put(&source, &format!("/{name}"), b"SOURCE").await;
    }
    let dest = volume("Dest");
    mkdir(&dest, "/dest").await;
    put(&dest, &format!("/dest/{CAFE_NFC}"), b"THE USER'S FILE").await;

    let sources = ["/a.txt".to_string(), "/b.txt".to_string(), format!("/{CAFE_NFD}")];
    let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
    let (outcome, _events, _state) = copy(
        &engine,
        &source,
        &sources,
        &dest,
        "/dest",
        ConflictResolution::Overwrite,
    )
    .await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(names_in(&dest, "/dest").await, sorted(&["a.txt", "b.txt", CAFE_NFC]));
    assert_eq!(bytes_at(&dest, &format!("/dest/{CAFE_NFC}")).await, b"SOURCE");
}

/// Two look-alikes and a source spelled like neither: which one it means is a
/// guess, so the copy refuses before writing anything there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_look_alikes_refuse_the_copy_instead_of_guessing() {
    let engine = TestOperations::new();
    let composed = "\u{e9}l\u{151}.txt";
    let decomposed = "e\u{301}lo\u{30b}.txt";
    let half = "\u{e9}lo\u{30b}.txt";
    let source = volume("Source");
    mkdir(&source, "/album").await;
    put(&source, &format!("/album/{half}"), b"SOURCE").await;
    let dest = volume("Dest");
    mkdir(&dest, "/album").await;
    put(&dest, &format!("/album/{composed}"), b"ONE").await;
    put(&dest, &format!("/album/{decomposed}"), b"TWO").await;

    let (outcome, _events, _state) =
        copy(&engine, &source, &["/album"], &dest, "/", ConflictResolution::Overwrite).await;

    assert!(
        matches!(
            outcome,
            Err(WriteFailure {
                error: WriteOperationError::DestinationExists { .. },
            })
        ),
        "the copy has to refuse, got {outcome:?}"
    );
    assert_eq!(names_in(&dest, "/album").await, sorted(&[composed, decomposed]));
    assert_eq!(bytes_at(&dest, &format!("/album/{composed}")).await, b"ONE");
    assert_eq!(bytes_at(&dest, &format!("/album/{decomposed}")).await, b"TWO");
}

/// A destination that asks for composed new names gets them at every depth: a
/// top-level file, a top-level folder, and a file inside the folder the copy
/// created.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_names_go_out_composed_where_the_destination_asks() {
    let engine = TestOperations::new();
    let source = volume("Source");
    mkdir(&source, &format!("/{FOTOK_NFD}")).await;
    put(&source, &format!("/{FOTOK_NFD}/{CAFE_NFD}"), b"DEEP").await;
    put(&source, &format!("/{CAFE_NFD}"), b"TOP").await;
    let dest = Arc::new(
        InMemoryVolume::new("Share")
            .with_space_info(10_000_000, 10_000_000)
            .with_composed_new_names(),
    );
    mkdir(&dest, "/dest").await;

    let sources = [format!("/{FOTOK_NFD}"), format!("/{CAFE_NFD}")];
    let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
    let (outcome, _events, _state) = copy(&engine, &source, &sources, &dest, "/dest", ConflictResolution::Stop).await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(names_in(&dest, "/dest").await, sorted(&[CAFE_NFC, FOTOK_NFC]));
    assert_eq!(
        names_in(&dest, &format!("/dest/{FOTOK_NFC}")).await,
        sorted(&[CAFE_NFC])
    );
    assert_eq!(bytes_at(&dest, &format!("/dest/{FOTOK_NFC}/{CAFE_NFC}")).await, b"DEEP");
}

/// Everywhere else a new name goes out exactly as the source spelled it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_names_go_out_as_given_by_default() {
    let engine = TestOperations::new();
    let source = volume("Source");
    put(&source, &format!("/{CAFE_NFD}"), b"TOP").await;
    let dest = volume("Dest");
    mkdir(&dest, "/dest").await;

    let (outcome, _events, _state) = copy(
        &engine,
        &source,
        &[&format!("/{CAFE_NFD}")],
        &dest,
        "/dest",
        ConflictResolution::Stop,
    )
    .await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(names_in(&dest, "/dest").await, sorted(&[CAFE_NFD]));
}

/// A Rename pick is a NEW name: it goes out spelled for the destination, and a
/// numbered name the folder already holds in another spelling is taken too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_pick_is_spelled_for_the_destination_and_skips_a_numbered_look_alike() {
    let engine = TestOperations::new();
    let source = volume("Source");
    mkdir(&source, "/album").await;
    put(&source, &format!("/album/{CAFE_NFD}"), b"SOURCE").await;
    let dest = Arc::new(
        InMemoryVolume::new("Share")
            .with_space_info(10_000_000, 10_000_000)
            .with_composed_new_names(),
    );
    mkdir(&dest, "/album").await;
    put(&dest, &format!("/album/{CAFE_NFD}"), b"THE USER'S FILE").await;
    put(&dest, "/album/cafe\u{301} (1).txt", b"AN OLDER COPY").await;

    let (outcome, _events, _state) = copy(&engine, &source, &["/album"], &dest, "/", ConflictResolution::Rename).await;

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(
        names_in(&dest, "/album").await,
        sorted(&[CAFE_NFD, "cafe\u{301} (1).txt", "caf\u{e9} (2).txt"]),
        "the pick skips the decomposed ` (1)` and lands composed"
    );
    assert_eq!(bytes_at(&dest, "/album/caf\u{e9} (2).txt").await, b"SOURCE");
}
