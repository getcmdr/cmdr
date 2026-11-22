//! A name the share already holds under another Unicode spelling, and renames,
//! against a real SMB server (require Docker SMB containers).
//!
//! The fixture's Samba stores a name's bytes exactly as sent and matches them
//! exactly, so `café` composed (NFC) and `café` decomposed (NFD, what macOS hands
//! out for many local files) are two entries to it. SMB answers `true` to
//! `Volume::composes_new_names`, so a genuinely new name lands composed here
//! (`a_new_name_lands_composed`), while an entry the share already holds
//! decomposed keeps its bytes
//! (`an_existing_decomposed_entry_keeps_its_exact_bytes`).
//!
//! The scenarios are backend-blind and live in
//! `network_look_alike_test_support.rs` (the compress one in
//! `network_archive_test_support.rs`), shared with `sftp_look_alike_test.rs`.
//! Unit twins: `transfer/volume/look_alike_tests.rs`.
//!
//! Case-only look-alikes aren't pinned here: the fixture runs `case sensitive =
//! auto`, so Samba folds case itself and the twin can't be made. That folding is
//! what `smb_integration_a_case_only_rename_asks_nothing_and_lands` pins: F2's
//! check finds the source under its new case and mustn't call it a conflict.

use super::network_archive_test_support::a_compress_replaces_a_look_alike_archive_in_place;
use super::network_look_alike_test_support::{
    a_bulk_rename_skips_a_look_alike, a_decomposed_file_onto_its_composed_twin_is_skipped,
    a_decomposed_folder_merges_into_its_composed_twin, a_new_name_lands_composed, a_rename_never_lands_on_a_taken_name,
    a_same_server_move_skips_a_look_alike, an_existing_decomposed_entry_keeps_its_exact_bytes,
    overwriting_a_composed_twin_leaves_one_entry,
};
use super::smb_test_support::fixture;
use crate::operations_host::app_engine;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_decomposed_file_onto_its_composed_twin_is_skipped_under_skip() {
    let (remote, dir) = fixture().await;
    a_decomposed_file_onto_its_composed_twin_is_skipped(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_overwriting_a_composed_twin_leaves_one_entry() {
    let (remote, dir) = fixture().await;
    overwriting_a_composed_twin_leaves_one_entry(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_decomposed_folder_merges_into_its_composed_twin() {
    let (remote, dir) = fixture().await;
    a_decomposed_folder_merges_into_its_composed_twin(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_new_name_lands_on_the_share_composed() {
    let (remote, dir) = fixture().await;
    a_new_name_lands_composed(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_an_existing_decomposed_entry_keeps_its_exact_bytes() {
    let (remote, dir) = fixture().await;
    an_existing_decomposed_entry_keeps_its_exact_bytes(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_same_share_move_skips_a_look_alike_instead_of_landing_beside_it() {
    let (remote, dir) = fixture().await;
    a_same_server_move_skips_a_look_alike(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_rename_never_lands_on_a_taken_name() {
    let (remote, dir) = fixture().await;
    a_rename_never_lands_on_a_taken_name(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_bulk_rename_skips_a_look_alike_and_names_new_ones_composed() {
    let (remote, dir) = fixture().await;
    a_bulk_rename_skips_a_look_alike(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_compress_replaces_a_look_alike_archive_in_place_and_names_new_ones_composed() {
    let (remote, dir) = fixture().await;
    a_compress_replaces_a_look_alike_archive_in_place(remote, dir).await;
}

/// QA 2026-10-14: F2 `c.txt` → `C.txt` on a share opened the "already exists"
/// dialog comparing the file with itself, because Samba (like a QNAP) finds
/// `C.txt` as `c.txt`. The pane's check has to see that the entry it found is the
/// source, and the rename then lands with the new case and the same bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_case_only_rename_asks_nothing_and_lands() {
    use super::network_safety_test_support::Registered;
    use super::network_semantics_test_support::{names_in, seed, try_read};
    use super::network_transfer_test_support::clean_deep;
    use cmdr_operation_log::types::Initiator;

    let (remote, dir) = fixture().await;
    seed(remote.as_ref(), &dir, &[("c.txt", b"abcd")]).await;
    let registered = Registered::new(&remote, "case-only");

    let check = app_engine()
        .check_rename_validity_impl(
            dir.to_string_lossy().into_owned(),
            "c.txt".into(),
            "C.txt".into(),
            registered.id.clone(),
        )
        .await
        .priced(|_| false);
    assert!(
        !check.has_conflict || check.is_case_only_rename,
        "the share's only `C.txt` is `c.txt` itself, so F2 asks nothing: {check:?}"
    );

    app_engine()
        .rename_managed(
            cmdr_operations::testing::app_path_of(&dir.join("c.txt")),
            cmdr_operations::testing::app_path_of(&dir.join("C.txt")),
            false,
            registered.id.clone(),
            Initiator::User,
        )
        .await
        .expect("the case-only rename lands");
    assert_eq!(names_in(remote.as_ref(), &dir).await, vec!["C.txt".to_string()]);
    assert_eq!(
        try_read(remote.as_ref(), &dir.join("C.txt")).await.as_deref(),
        Some(&b"abcd"[..])
    );

    clean_deep(remote.as_ref(), &dir).await;
    registered.leave();
}
