//! The shared `Volume` conformance promises, asserted against this OS's `LocalVolume`
//! (`LocalPosixVolume` on macOS and Linux, `LocalWindowsVolume` on real NTFS on Windows).
//!
//! These live apart from `local_volume_test.rs` because they assert something different in
//! kind: not how this backend behaves, but that it keeps the contracts
//! `cmdr_fs::volume::conformance` holds EVERY backend to. Every other backend keeps its
//! conformance cells in a file of their own (`cmdr-smb`'s and `cmdr-sftp`'s
//! `volume::conformance_test`, `cmdr-mtp`'s `volume::conformance_test`); this is that file for
//! the local one.
//!
//! A promise the Windows volume doesn't keep yet is ignored there with the piece of task 35
//! that owns it, never left out and never passing by a default: an ignored cell is a named
//! debt, a missing one is a silent pass.
//!
//! ❗ The local volume is the backend the other suites' fixtures are compared against, so a
//! contract it stops keeping is one the whole suite stops noticing.

use crate::test_support::{at, volume_at};
use cmdr_fs::testing::AppTestDir;
use cmdr_fs::volume::conformance;
use cmdr_fs::volume::{StreamLength, Volume, WriteMode};
use std::path::Path;

/// The shared `Volume::delete` non-recursion assertion. POSIX gets it for free from
/// `std::fs::remove_dir`'s `ENOTEMPTY`, which is exactly why it's worth pinning: "free" is
/// what makes a contract invisible until a backend that doesn't get it free comes along.
#[tokio::test]
async fn delete_honors_the_shared_non_recursion_contract() {
    let test_dir = AppTestDir::new("delete_non_recursion_test");
    let volume = volume_at(&test_dir);

    volume.create_directory(&at(&volume, "album")).await.unwrap();
    volume
        .create_file(&at(&volume, "album/keep.txt"), b"content")
        .await
        .unwrap();

    conformance::assert_delete_leaves_a_non_empty_dir_intact(&volume, Path::new("album"), "keep.txt").await;
}

/// The batch delete a move's source sweep uses, through the trait's default.
#[tokio::test]
async fn delete_files_honors_the_shared_batch_contract() {
    let test_dir = AppTestDir::new("delete_files_batch_test");
    let volume = volume_at(&test_dir);
    volume.create_directory(&at(&volume, "level")).await.unwrap();
    for name in ["a.txt", "b.txt", "kept.txt"] {
        volume
            .create_file(&at(&volume, &format!("level/{name}")), b"bytes")
            .await
            .unwrap();
    }

    conformance::assert_delete_files_removes_exactly_what_it_names(
        &volume,
        [Path::new("level/a.txt"), Path::new("level/b.txt")],
        Path::new("level/kept.txt"),
    )
    .await;
}

/// The shared `Volume::rename` no-clobber assertion. POSIX earns it with
/// `renamex_np(RENAME_EXCL)` / `renameat2(RENAME_NOREPLACE)`, Windows with `MoveFileExW`
/// without `MOVEFILE_REPLACE_EXISTING`: each one kernel operation with no TOCTOU window, a
/// different mechanism from every other backend's, which is the whole reason the promise is
/// asserted rather than assumed.
#[tokio::test]
async fn rename_honors_the_shared_no_clobber_contract() {
    let test_dir = AppTestDir::new("rename_no_clobber_conformance_test");
    let volume = volume_at(&test_dir);

    volume.create_file(&at(&volume, "source.txt"), b"source").await.unwrap();
    volume
        .create_file(&at(&volume, "target.txt"), b"the user's target file")
        .await
        .unwrap();

    conformance::assert_rename_refuses_an_existing_destination(
        &volume,
        Path::new("source.txt"),
        Path::new("target.txt"),
    )
    .await;
}

/// The shared forced-rename assertion. POSIX earns it from the kernel: `rename(2)` answers
/// `EISDIR` for a file onto a folder and `ENOTEMPTY` for a folder onto one that holds
/// something.
#[tokio::test]
async fn forced_rename_honors_the_shared_non_empty_folder_contract() {
    let test_dir = AppTestDir::new("forced_rename_non_empty_folder_conformance_test");
    let volume = volume_at(&test_dir);

    volume.create_file(&at(&volume, "source.txt"), b"source").await.unwrap();
    volume.create_directory(&at(&volume, "source-dir")).await.unwrap();
    volume.create_directory(&at(&volume, "album")).await.unwrap();
    volume
        .create_file(&at(&volume, "album/source.txt"), b"the user's photo")
        .await
        .unwrap();

    for from in ["source.txt", "source-dir"] {
        conformance::assert_forced_rename_keeps_a_non_empty_dir(&volume, Path::new(from), Path::new("album")).await;
    }
}

/// The shared `Volume::create_file` no-clobber assertion, earned with
/// `OpenOptions::create_new(true)`; a plain `std::fs::write` one refactor away would truncate
/// instead, with the New File command still reporting success.
#[tokio::test]
async fn create_file_honors_the_shared_no_clobber_contract() {
    let test_dir = AppTestDir::new("create_file_no_clobber_conformance_test");
    let volume = volume_at(&test_dir);

    volume
        .create_file(&at(&volume, "notes.txt"), b"the user's notes")
        .await
        .unwrap();

    conformance::assert_create_file_refuses_to_clobber(&volume, Path::new("notes.txt"), b"new").await;
}

/// The shared `write_from_stream(CreateNew)` no-clobber assertion, earned with
/// `OpenOptions::create_new(true)` (`O_EXCL`).
#[tokio::test]
async fn write_from_stream_create_new_honors_the_shared_no_clobber_contract() {
    let test_dir = AppTestDir::new("write_create_new_conformance_test");
    let volume = volume_at(&test_dir);

    volume
        .create_file(&at(&volume, "notes.txt"), b"the user's notes")
        .await
        .unwrap();

    conformance::assert_write_from_stream_create_new_refuses_to_clobber(
        &volume,
        Path::new("notes.txt"),
        Path::new("fresh.txt"),
        b"new",
    )
    .await;
}

/// The shared date assertions. Every copy landing on local disk from another volume goes
/// through `write_from_stream`, so this is the cell that keeps a phone's photos dated when
/// they land in `~/Pictures`.
#[tokio::test]
async fn a_copy_keeps_the_source_date_per_the_shared_contract() {
    let test_dir = AppTestDir::new("dated_write_conformance_test");
    let volume = volume_at(&test_dir);

    conformance::assert_write_from_stream_keeps_the_source_date(
        &volume,
        Path::new("dated.txt"),
        std::time::Duration::ZERO,
    )
    .await;
    conformance::assert_read_stream_reports_the_listed_date(&volume, Path::new("dated.txt")).await;
    conformance::assert_landed_time_agrees_with_get_metadata(&volume, Path::new("landed.txt")).await;
}

/// Local disk lists no date its `supported_dates` doesn't claim, for a file, a folder, and (on
/// macOS and Linux) a link. On macOS the temp dir is APFS, so the claim is both dates; Linux
/// and Windows record no added date, and NTFS records a birth time.
#[tokio::test]
async fn listing_dates_honor_the_shared_claim_contract() {
    let test_dir = AppTestDir::new("listed_dates_conformance_test");
    let volume = volume_at(&test_dir);
    volume.create_directory(&at(&volume, "dated")).await.unwrap();
    volume.create_directory(&at(&volume, "dated/folder")).await.unwrap();
    volume
        .create_file(&at(&volume, "dated/file.txt"), b"bytes")
        .await
        .unwrap();
    // A file symlink on Windows needs Developer Mode or an elevated token.
    #[cfg(unix)]
    cmdr_fs::testing::symlink("file.txt", test_dir.native().join("dated/link")).unwrap();

    conformance::assert_listing_dates_are_claimed(&volume, Path::new("dated")).await;
    let claimed = volume.supported_dates(&at(&volume, "dated")).await;
    #[cfg(target_os = "macos")]
    assert_eq!(
        claimed,
        cmdr_fs::volume::SupportedDates::ALL,
        "an APFS folder carries both dates"
    );
    #[cfg(target_os = "linux")]
    assert!(!claimed.added, "Linux records no added date");
    #[cfg(target_os = "windows")]
    {
        assert!(claimed.created, "NTFS records a birth time");
        assert!(!claimed.added, "Windows records no added date");
    }
}

/// A copy onto local disk dates the folders it created, once their contents landed.
#[tokio::test]
async fn apply_metadata_honors_the_shared_folder_date_contract() {
    let test_dir = AppTestDir::new("folder_date_conformance_test");
    let volume = volume_at(&test_dir);

    conformance::assert_apply_metadata_dates_a_folder(&volume, Path::new("dated"), std::time::Duration::ZERO).await;
}

/// Local disk keeps and reports what its caps claim, on a file and on a folder: the creation
/// date (macOS and Windows: Linux can't set a birth time) and the mode (macOS and Linux).
#[tokio::test]
async fn local_disk_honors_the_shared_metadata_contract() {
    let test_dir = AppTestDir::new("metadata_conformance_test");
    let volume = volume_at(&test_dir);

    conformance::assert_write_from_stream_keeps_metadata(&volume, Path::new("born.txt")).await;
    conformance::assert_read_stream_reports_the_listed_metadata(&volume, Path::new("born.txt")).await;
    conformance::assert_apply_metadata_keeps(&volume, Path::new("born")).await;
}

#[tokio::test]
async fn unknown_write_streams_all_bytes_and_reports_the_accepted_count() {
    let test_dir = AppTestDir::new("unknown_write_conformance_test");
    let volume = volume_at(&test_dir);
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let (cancel_tx, _cancel_rx) = tokio::sync::oneshot::channel();
    tx.send(Ok(b"first".to_vec())).await.unwrap();
    let sender = tokio::spawn(async move {
        tx.send(Ok(b"-second".to_vec())).await.unwrap();
    });
    let progress = std::sync::Mutex::new(Vec::new());
    let written = volume
        .write_from_stream(
            &at(&volume, "notes.txt"),
            WriteMode::CreateNew,
            StreamLength::Unknown,
            Box::new(cmdr_fs::volume::ChannelReadStream::new(
                rx,
                cancel_tx,
                StreamLength::Unknown,
            )),
            &|tick| {
                progress.lock().unwrap().push(tick);
                std::ops::ControlFlow::Continue(())
            },
        )
        .await;
    sender.await.unwrap();

    assert_eq!(written.unwrap().bytes, 12);
    assert_eq!(
        std::fs::read(test_dir.native().join("notes.txt")).unwrap(),
        b"first-second"
    );
    let progress = progress.lock().unwrap();
    assert_eq!(progress.last().unwrap().bytes_written, 12);
    assert_eq!(progress.last().unwrap().expected_length, StreamLength::Unknown);
}

/// The shared `Volume::create_directory_all` honesty assertion, over the trait's default walk
/// composed from the volume's own `exists` + `create_directory`.
#[tokio::test]
async fn create_directory_all_honors_the_shared_honesty_contract() {
    let test_dir = AppTestDir::new("create_directory_all_honesty_conformance_test");
    let volume = volume_at(&test_dir);

    volume.create_directory(&at(&volume, "album")).await.unwrap();

    conformance::assert_create_directory_all_reports_an_existing_dir_honestly(&volume, Path::new("album")).await;
}

/// The shared file-in-the-way assertion, over the trait's default walk. POSIX answers a
/// create under a file with `ENOTDIR`, which reaches the user as a generic refusal unless the
/// walk names the file.
#[tokio::test]
async fn create_directory_all_honors_the_shared_file_in_the_way_contract() {
    let test_dir = AppTestDir::new("create_directory_all_file_in_the_way_conformance_test");
    let volume = volume_at(&test_dir);

    volume
        .create_file(&at(&volume, "notes"), b"the user's notes")
        .await
        .unwrap();

    conformance::assert_create_directory_all_refuses_a_file_in_the_way(&volume, Path::new("notes")).await;
}

/// The shared through-a-link assertion. ❗ The cell that keeps the refusal above from
/// overreaching: `/tmp` and `/var` are links on macOS, so a walk that judged an occupied name
/// by `lstat` would refuse most of the disk.
#[tokio::test]
async fn create_directory_all_honors_the_shared_through_a_link_contract() {
    let test_dir = AppTestDir::new("create_directory_all_through_a_link_conformance_test");
    let volume = volume_at(&test_dir);

    volume.create_directory(&at(&volume, "real")).await.unwrap();
    link_to_folder(test_dir.native(), "link", "real");

    conformance::assert_create_directory_all_goes_through_a_link_to_a_folder(
        &volume,
        Path::new("link"),
        Path::new("real"),
    )
    .await;
}

/// The shared link-delete assertion: a link to a folder is a leaf, and its delete leaves the
/// target alone (#505).
#[tokio::test]
async fn delete_of_a_link_to_a_folder_honors_the_shared_contract() {
    let test_dir = AppTestDir::new("delete_of_a_link_conformance_test");
    let volume = volume_at(&test_dir);

    volume.create_directory(&at(&volume, "real")).await.unwrap();
    volume
        .create_file(&at(&volume, "real/keep.txt"), b"keep")
        .await
        .unwrap();
    link_to_folder(test_dir.native(), "link", "real");

    conformance::assert_delete_of_a_link_to_a_folder_removes_the_link_alone(
        &volume,
        Path::new("link"),
        Path::new("real"),
        "keep.txt",
    )
    .await;
}

/// The shared same-volume link-move assertion: a link to a folder lists as a link and renames
/// as one, its target left alone (#570).
#[tokio::test]
async fn a_link_to_a_folder_moves_as_the_link_per_the_shared_contract() {
    let test_dir = AppTestDir::new("link_move_conformance_test");
    let volume = volume_at(&test_dir);

    volume.create_directory(&at(&volume, "real")).await.unwrap();
    volume
        .create_file(&at(&volume, "real/keep.txt"), b"keep")
        .await
        .unwrap();
    volume.create_directory(&at(&volume, "moved")).await.unwrap();
    link_to_folder(test_dir.native(), "link", "real");

    conformance::assert_a_link_to_a_folder_moves_as_the_link(
        &volume,
        Path::new("link"),
        Path::new("moved/link"),
        Path::new("real"),
        "keep.txt",
    )
    .await;
}

/// The shared writability-declaration assertion: `is_writable()` and the mutations the volume
/// offers say the same thing.
#[tokio::test]
async fn is_writable_honors_the_shared_declaration_contract() {
    let test_dir = AppTestDir::new("is_writable_declaration_conformance_test");
    let volume = volume_at(&test_dir);

    conformance::assert_writability_matches_the_mutations_offered(&volume, Path::new("scratch")).await;
}

/// The shared export-handshake assertion: a volume that streams its bytes must say
/// `supports_export()`.
#[tokio::test]
async fn export_honors_the_shared_handshake_contract() {
    let test_dir = AppTestDir::new("export_handshake_conformance_test");
    let volume = volume_at(&test_dir);
    let content = b"the bytes a copy would move";
    volume.create_file(&at(&volume, "exported.txt"), content).await.unwrap();

    conformance::assert_export_matches_the_bytes_offered(&volume, Path::new("exported.txt"), content).await;
}

/// The shared server-side-copy handshake: a local disk has no server to copy for it, so it
/// claims nothing and answers `NotSupported`. That `false` is what keeps every
/// local-destination transfer from staging a server-side copy per file (ERR-PTRGY).
#[tokio::test]
async fn copy_within_honors_the_shared_handshake_contract() {
    let test_dir = AppTestDir::new("copy_within_handshake_conformance_test");
    let volume = volume_at(&test_dir);
    volume
        .create_file(&at(&volume, "original.txt"), b"bytes")
        .await
        .unwrap();

    conformance::assert_copy_within_matches_what_is_offered(
        &volume,
        Path::new("original.txt"),
        Path::new("duplicate.txt"),
    )
    .await;
}

/// The shared `NotFound`-payload assertion: the string the frontend renders as the missing
/// file's name really is its path.
///
/// The local volume earns it from `VolumeError::from_io_at` (POSIX) and the located app path
/// (Windows), applied at every site that touches the filesystem. There is deliberately no
/// `From<std::io::Error>` to reach for, so a new `?` site can't quietly go back to putting the
/// errno's sentence where the frontend renders a filename.
#[tokio::test]
async fn not_found_honors_the_shared_path_payload_contract() {
    let test_dir = AppTestDir::new("not_found_payload_conformance_test");
    let volume = volume_at(&test_dir);

    conformance::assert_not_found_carries_the_path(&volume, Path::new("no-such-file.txt")).await;
}

/// The shared conflict-scan assertion. The local volume gets it for free from the per-item
/// probe, which finds nothing under a destination directory that isn't there. "Free" is what
/// makes the contract invisible until a backend that has to spend a listing on it comes
/// along, and two of them did.
#[tokio::test]
async fn conflict_scan_honors_the_shared_missing_destination_contract() {
    let test_dir = AppTestDir::new("conflict_scan_missing_destination_conformance_test");
    let volume = volume_at(&test_dir);

    conformance::assert_conflict_scan_reads_a_missing_destination_as_empty(&volume, Path::new("not-created-yet")).await;
}

#[tokio::test]
#[cfg_attr(
    target_os = "windows",
    ignore = "task 35, symlinks and junctions: can_hold_links is false until file vs dir symlinks land"
)]
async fn a_link_lands_verbatim() {
    let dir = AppTestDir::new("conformance-link-landing");
    let volume = volume_at(&dir);
    conformance::assert_a_link_lands_verbatim(&volume, Path::new("/")).await;
}

/// The contract under `user.*`, a name both macOS and Linux take. The reading of a Finder tag
/// lives with the rest of the tag rules (`file_system/tags_write_test.rs`).
#[tokio::test]
#[cfg_attr(
    target_os = "windows",
    ignore = "task 35, ADS-backed xattrs: supports_xattrs is false until alternate data streams back them"
)]
async fn xattrs_keep_the_shared_contract() {
    let test_dir = AppTestDir::new("xattrs_contract_test");
    let volume = volume_at(&test_dir);
    volume.create_file(&at(&volume, "plain.txt"), b"x").await.unwrap();
    conformance::assert_xattrs_keep_their_promises(&volume, Path::new("plain.txt"), Path::new("gone.txt")).await;
}

/// A link to the folder `target` at `link`, both names inside `dir`: a symlink on macOS and
/// Linux, a junction on Windows, where a symlink needs Developer Mode or an elevated token and
/// a junction needs no privilege. Either is the folder link a volume must treat as a leaf.
pub(crate) fn link_to_folder(dir: &Path, link: &str, target: &str) {
    #[cfg(unix)]
    cmdr_fs::testing::symlink(dir.join(target), dir.join(link)).expect("a symlink to the folder");
    #[cfg(target_os = "windows")]
    junction(dir, link, target);
}

/// A junction to `target` at `link`, both inside `dir`, through `mklink /J`, since the volume
/// can't make a link itself yet (`create_symlink`, task 35).
#[cfg(target_os = "windows")]
pub(crate) fn junction(dir: &Path, link: &str, target: &str) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dir.join(link))
        .arg(dir.join(target))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("run mklink");
    assert!(status.success(), "mklink /J {link} {target}");
}
