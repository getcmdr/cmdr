//! The cross-backend promises (`cmdr_fs::volume::conformance`) against the
//! fixture's `/insecure` export: the write side's cells.
//!
//! ❗ **The server is real knfsd, and that is the point.** v3 `RENAME` replaces
//! its target and `CREATE UNCHECKED` truncates one, so a `rename` or a
//! `create_file` wired straight to the procedure passes every fake-server cell
//! that doesn't think to look, and silently clobbers here.
//!
//! Not run here, and why:
//!
//! - **The link cells** (`assert_create_directory_all_goes_through_a_link_to_a_folder`,
//!   `assert_delete_of_a_link_to_a_folder_removes_the_link_alone`,
//!   `assert_a_link_to_a_folder_moves_as_the_link`): this backend makes no
//!   links (no `SYMLINK`), and the seeded tree's only links lead to a file and
//!   out of the export. The link-delete rule is pinned against `fake_nfs.rs`
//!   (`mutation_test.rs`).

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic};
use std::time::Duration;

use cmdr_fs::volume::{ChannelReadStream, StreamLength, Volume, WriteMode, conformance};

use super::NfsVolume;
use super::testing::*;

const FIXTURE: &str = "nfs-servers/start.sh (nfs-fixture)";

/// The fixture's `/insecure` export, plus a scratch folder of this cell's own.
async fn insecure_with_scratch(what: &str) -> (NfsVolume, AppPathBuf) {
    let volume = connect_insecure().await;
    let dir = scratch_dir_at(&volume, what).await;
    (volume, dir)
}

/// `name` in `dir`.
fn under(dir: &AppPath, name: &str) -> AppPathBuf {
    dir.join(AppName::from_app_spelling(name).expect("a plain name"))
        .expect("joins")
}

/// NFSv3 attributes have no creation date (`mapping.rs`), and the default
/// `supported_dates` claims none.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn listing_dates_honor_the_shared_claim_contract() {
    let (volume, dir) = insecure_with_scratch("listed-dates").await;
    volume
        .create_file(&under(&dir, "dated.txt"), b"bytes")
        .await
        .expect(FIXTURE);
    volume.create_directory(&under(&dir, "folder")).await.expect(FIXTURE);

    conformance::assert_listing_dates_are_claimed(&volume, &std_path_of(&dir)).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn a_forceless_rename_refuses_an_existing_destination() {
    // ❗ THE data-safety cell of the write path: v3 `RENAME` replaces.
    let (volume, dir) = insecure_with_scratch("rename-no-clobber").await;
    let source = under(&dir, "source.txt");
    let target = under(&dir, "target.txt");
    volume.create_file(&source, b"source").await.expect(FIXTURE);
    volume
        .create_file(&target, b"the user's target file")
        .await
        .expect(FIXTURE);

    conformance::assert_rename_refuses_an_existing_destination(&volume, &std_path_of(&source), &std_path_of(&target))
        .await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn a_forced_rename_keeps_a_non_empty_folder() {
    let (volume, dir) = insecure_with_scratch("forced-rename-full-folder").await;
    let source = under(&dir, "source.txt");
    let source_dir = under(&dir, "source-dir");
    let album = under(&dir, "album");
    volume.create_file(&source, b"source").await.expect(FIXTURE);
    volume.create_directory(&source_dir).await.expect(FIXTURE);
    volume.create_directory(&album).await.expect(FIXTURE);
    volume
        .create_file(&under(&album, "source.txt"), b"the user's photo")
        .await
        .expect(FIXTURE);

    for from in [&source, &source_dir] {
        conformance::assert_forced_rename_keeps_a_non_empty_dir(&volume, &std_path_of(from), &std_path_of(&album))
            .await;
    }

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn create_file_refuses_to_clobber() {
    // The refusal is `CREATE GUARDED`'s; `UNCHECKED` would truncate.
    let (volume, dir) = insecure_with_scratch("create-file-no-clobber").await;
    let notes = under(&dir, "notes.txt");
    volume.create_file(&notes, b"the user's notes").await.expect(FIXTURE);

    conformance::assert_create_file_refuses_to_clobber(&volume, &std_path_of(&notes), b"new").await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn write_from_stream_create_new_refuses_to_clobber() {
    let (volume, dir) = insecure_with_scratch("write-create-new-no-clobber").await;
    let notes = under(&dir, "notes.txt");
    volume.create_file(&notes, b"the user's notes").await.expect(FIXTURE);

    conformance::assert_write_from_stream_create_new_refuses_to_clobber(
        &volume,
        &std_path_of(&notes),
        &std_path_of(&under(&dir, "fresh.txt")),
        b"new",
    )
    .await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn unknown_write_is_refused_before_io() {
    let (volume, dir) = insecure_with_scratch("unknown-length").await;
    let notes = under(&dir, "notes.txt");
    let original = b"the user's notes";
    volume.create_file(&notes, original).await.expect(FIXTURE);

    conformance::assert_unknown_write_is_refused_before_io(&volume, &std_path_of(&notes), original).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn create_directory_all_reports_an_existing_directory_honestly() {
    // ❗ `Created` is a promise the transfer driver SPENDS: on it, it skips the
    // per-file conflict probe for everything it writes inside.
    let (volume, dir) = insecure_with_scratch("mkdir-p-honesty").await;
    let album = under(&dir, "album");
    volume.create_directory(&album).await.expect(FIXTURE);

    conformance::assert_create_directory_all_reports_an_existing_dir_honestly(&volume, &std_path_of(&album)).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn create_directory_all_refuses_a_file_in_the_way() {
    // `MKDIR` answers `EXIST` for a file and a folder alike, and the walk
    // under a file names the FILE, ❌ not the folder asked for.
    let (volume, dir) = insecure_with_scratch("mkdir-p-file-in-the-way").await;
    let notes = under(&dir, "notes");
    volume.create_file(&notes, b"the user's notes").await.expect(FIXTURE);

    conformance::assert_create_directory_all_refuses_a_file_in_the_way(&volume, &std_path_of(&notes)).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn delete_leaves_a_non_empty_directory_intact() {
    let (volume, dir) = insecure_with_scratch("delete-non-recursive").await;
    let album = under(&dir, "album");
    volume.create_directory(&album).await.expect(FIXTURE);
    volume
        .create_file(&under(&album, "keep.txt"), b"content")
        .await
        .expect(FIXTURE);

    conformance::assert_delete_leaves_a_non_empty_dir_intact(&volume, &std_path_of(&album), "keep.txt").await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn delete_removes_exactly_what_it_names() {
    let (volume, dir) = insecure_with_scratch("delete-exact").await;
    let (one, two, kept) = (under(&dir, "one.txt"), under(&dir, "two.txt"), under(&dir, "kept.txt"));
    for file in [&one, &two, &kept] {
        volume.create_file(file, b"x").await.expect(FIXTURE);
    }

    conformance::assert_delete_files_removes_exactly_what_it_names(
        &volume,
        [&std_path_of(&one), &std_path_of(&two)],
        &std_path_of(&kept),
    )
    .await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn writability_matches_the_mutations_offered() {
    let (volume, dir) = insecure_with_scratch("writability").await;

    conformance::assert_writability_matches_the_mutations_offered(&volume, &std_path_of(&under(&dir, "unborn"))).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn export_matches_the_bytes_offered() {
    let (volume, dir) = insecure_with_scratch("export-handshake").await;
    let file = under(&dir, "exported.txt");
    let content = b"the bytes a copy would move";
    volume.create_file(&file, content).await.expect(FIXTURE);

    conformance::assert_export_matches_the_bytes_offered(&volume, &std_path_of(&file), content).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn copy_within_matches_what_is_offered() {
    // v3 has no server-side copy, so this holds the backend to saying so.
    let (volume, dir) = insecure_with_scratch("copy-within-handshake").await;
    let from = under(&dir, "original.txt");
    volume.create_file(&from, b"copied on the server").await.expect(FIXTURE);

    conformance::assert_copy_within_matches_what_is_offered(
        &volume,
        &std_path_of(&from),
        &std_path_of(&under(&dir, "duplicate.txt")),
    )
    .await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn a_copy_keeps_the_source_date() {
    let (volume, dir) = insecure_with_scratch("dated-copy").await;
    let dated = under(&dir, "dated.txt");

    conformance::assert_write_from_stream_keeps_the_source_date(&volume, &std_path_of(&dated), Duration::ZERO).await;
    conformance::assert_read_stream_reports_the_listed_date(&volume, &std_path_of(&dated)).await;
    conformance::assert_landed_time_agrees_with_get_metadata(&volume, &std_path_of(&under(&dir, "landed.txt"))).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn apply_metadata_dates_a_folder() {
    let (volume, dir) = insecure_with_scratch("dated-folder").await;

    conformance::assert_apply_metadata_dates_a_folder(&volume, &std_path_of(&under(&dir, "dated")), Duration::ZERO)
        .await;

    clean_deep(&volume, &dir).await;
}

/// Holds the server to the caps: a file's and a folder's mode and a folder's
/// date, and nothing else, so the day another class lands the caps say so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_honors_the_shared_metadata_contract() {
    let (volume, dir) = insecure_with_scratch("metadata-contract").await;
    let born = under(&dir, "born.txt");

    conformance::assert_write_from_stream_keeps_metadata(&volume, &std_path_of(&born)).await;
    conformance::assert_read_stream_reports_the_listed_metadata(&volume, &std_path_of(&born)).await;
    conformance::assert_apply_metadata_keeps(&volume, &std_path_of(&under(&dir, "born"))).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn not_found_carries_the_path() {
    let (volume, dir) = insecure_with_scratch("not-found-path").await;

    conformance::assert_not_found_carries_the_path(&volume, &std_path_of(&under(&dir, "no-such-file.txt"))).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn conflict_scan_reads_a_missing_destination_as_empty() {
    let (volume, dir) = insecure_with_scratch("conflict-scan-missing-dest").await;

    conformance::assert_conflict_scan_reads_a_missing_destination_as_empty(
        &volume,
        &std_path_of(&under(&dir, "not-created-yet")),
    )
    .await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn a_batch_scan_stops_when_it_is_told_to() {
    let (volume, dir) = insecure_with_scratch("scan-stop-told").await;
    volume.create_file(&under(&dir, "a.txt"), b"a").await.expect(FIXTURE);

    conformance::assert_batch_scan_stops_when_told(&volume, &std_path_of(&dir)).await;

    clean_deep(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn a_batch_scan_asks_its_boundary_inside_the_walk() {
    let (volume, dir) = insecure_with_scratch("scan-stop-per-entry").await;
    volume.create_file(&under(&dir, "a.txt"), b"a").await.expect(FIXTURE);
    volume.create_file(&under(&dir, "b.txt"), b"bb").await.expect(FIXTURE);

    conformance::assert_batch_scan_asks_inside_the_walk(&volume, &std_path_of(&dir), 3).await;

    clean_deep(&volume, &dir).await;
}

/// A file bigger than one COMMIT span, streamed `UNSTABLE` and read back
/// byte-exact, so the real server's WRITE, COMMIT, and verifier answers are
/// the ones exercised (the fixture exports `sync`, so knfsd honours
/// `UNSTABLE`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn a_file_past_one_commit_span_round_trips_byte_exact() {
    let (volume, dir) = insecure_with_scratch("big-round-trip").await;
    let bytes: Vec<u8> = (0..9 * 1024 * 1024 + 4321).map(|at: usize| (at % 251) as u8).collect();
    let file = under(&dir, "big.bin");
    let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(4);
    let (cancel_tx, _cancel_rx) = tokio::sync::oneshot::channel();
    let feed = bytes.clone();
    tokio::spawn(async move {
        for chunk in feed.chunks(256 * 1024) {
            if chunk_tx.send(Ok(chunk.to_vec())).await.is_err() {
                return;
            }
        }
    });
    let length = StreamLength::Known(bytes.len() as u64);
    let stream = ChannelReadStream::new(chunk_rx, cancel_tx, length);

    let written = volume
        .write_from_stream(&file, WriteMode::CreateNew, length, Box::new(stream), &|_| {
            std::ops::ControlFlow::Continue(())
        })
        .await
        .expect(FIXTURE);
    let read = volume.read_range(&file, 0, bytes.len()).await.expect(FIXTURE);

    assert_eq!(written.bytes, bytes.len() as u64);
    assert!(read == bytes, "the file must read back as it was written");
    clean_deep(&volume, &dir).await;
}
