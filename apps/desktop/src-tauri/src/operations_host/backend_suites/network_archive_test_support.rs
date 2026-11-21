//! Archives living on a network server, written once for every backend.
//!
//! A zip on a server has no local path, so every archive operation reaches it
//! through the volume: browsing and extracting ride `read_range`, and every edit
//! (delete an entry, copy files in, compress a new zip) is pull → apply locally
//! → upload to a staging sibling → swap. These scenarios prove each leg against
//! a live server and re-read the result THROUGH the server, so a corrupt swap
//! fails loudly. The in-memory twins are `archive/volume_test.rs`'s
//! `remote_backed_archive_*` and `archive_edit::remote_tests`.
//!
//! Same contract as `network_transfer_test_support.rs`: a live volume and a
//! scratch directory in, the directory gone afterwards, and the `#[tokio::test]`
//! cells in the backend files the lane selects by name prefix.

use cmdr_fs::app_path::AppPath;
use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_archive::mutator::{self, Changeset, MutationHooks};
use cmdr_archive::{ArchiveFormat, ArchiveVolume};
use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;

use super::network_look_alike_test_support::{CAFE_NFC, CAFE_NFD, RESUME_NFC, RESUME_NFD, assert_composes_new_names};
use super::network_safety_test_support::Registered;
use super::network_semantics_test_support::{Transfer, local_volume, names_in, seed, transfer, try_read};
use super::network_transfer_test_support::{assert_no_staging_litter, clean_deep, read_all};
use crate::file_system::volume::manager::get_volume_manager;
use crate::operations_host::app_engine;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_operation_log::types::Initiator;
use cmdr_operations::testing::new_operation_state;
use cmdr_operations::testing::{CollectorEventSink, EditError, OperationIntent, pull_apply_upload_swap};
use cmdr_operations::{ConflictResolution, OperationEventSink, WriteOperationPhase};

/// How long an archive op gets to reach its terminal event. Under the 8 s
/// nextest cap, so a hang says what it was waiting for.
const ARCHIVE_BUDGET: Duration = Duration::from_secs(6);

/// A no-op `MutationHooks` for the mutator (never pauses or cancels here).
struct NoHooks;
impl MutationHooks for NoHooks {}

/// Two entries: a stored one to keep and a deflated one in a subfolder to drop,
/// so a browse runs the synthetic-folder path and a delete edit has something to
/// remove and something to keep verbatim.
fn two_entry_zip() -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    w.start_file("keep.txt", stored).expect("start the stored entry");
    w.write_all(b"keep me").expect("write the stored entry");
    w.start_file("dir/drop.txt", deflated)
        .expect("start the deflated entry");
    w.write_all(b"delete me from the server")
        .expect("write the deflated entry");
    w.finish().expect("finish the fixture zip").into_inner()
}

/// Reads a zip back off the server and parses it into `name -> bytes`, so an
/// assertion checks the archive the SERVER holds rather than a local copy.
async fn read_server_zip(remote: &dyn Volume, path: &Path) -> HashMap<String, Vec<u8>> {
    let bytes = read_all(remote, path).await;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("the server's zip must parse");
    let mut out = HashMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("zip entry");
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().expect("a decodable name").into_owned();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).expect("zip entry bytes");
        out.insert(name, buf);
    }
    out
}

/// Seeds the two-entry zip at `dir/archive.zip` and returns its path.
async fn seed_zip(remote: &dyn Volume, dir: &Path) -> PathBuf {
    seed_zip_at(remote, dir).await.0
}

/// [`seed_zip`], with the zip's app path beside it (the spelling the volume
/// and the route take).
async fn seed_zip_at(remote: &dyn Volume, dir: &Path) -> (PathBuf, cmdr_fs::app_path::AppPathBuf) {
    let zip_path = dir.join("archive.zip");
    let zip = AppPath::legacy_unchecked(&zip_path).into_owned();
    remote
        .create_file(&zip, &two_entry_zip())
        .await
        .expect("seeding the zip on the server");
    (zip_path, zip)
}

/// Waits for an archive op's terminal event and fails loudly on an error one.
async fn await_archive_op(events: &CollectorEventSink, what: &str) {
    crate::test_support::wait_until_async(ARCHIVE_BUDGET, what, || {
        !events.complete.lock_ignore_poison().is_empty() || !events.errors.lock_ignore_poison().is_empty()
    })
    .await;
    let errors = events.errors.lock_ignore_poison();
    assert!(errors.is_empty(), "{what}: the op reported {errors:?}");
}

/// A zip on the server BROWSES and EXTRACTS through ranged reads: the root
/// lists the synthetic folder and the file, and an extract-out through the copy
/// engine lands both entries' bytes on local disk.
pub(super) async fn a_zip_on_the_server_browses_and_extracts(remote: Arc<dyn Volume>, dir: PathBuf) {
    let zip_path = seed_zip(remote.as_ref(), &dir).await;
    assert!(
        !remote.supports_local_fs_access(),
        "this scenario is about a zip with no local path, read through the volume"
    );
    let archive: Arc<dyn Volume> = Arc::new(ArchiveVolume::new(
        Arc::clone(&remote),
        zip_path.clone(),
        ArchiveFormat::Zip,
        VolumeHost::detached(),
    ));

    let root: Vec<String> = archive
        .list_directory(&AppPath::legacy_unchecked(Path::new("")), None)
        .await
        .expect("listing the zip's root")
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(root, vec!["dir", "keep.txt"], "the zip's root, folders first");

    let (_local_dir, local) = local_volume("zip-extract");
    transfer(
        "zip-extract",
        Transfer::Copy,
        &archive,
        &[PathBuf::from("keep.txt"), PathBuf::from("dir")],
        &local,
        Path::new(""),
        ConflictResolution::Stop,
    )
    .await;
    assert_eq!(
        try_read(local.as_ref(), Path::new("keep.txt")).await.as_deref(),
        Some(&b"keep me"[..]),
        "a STORED entry extracts byte for byte"
    );
    assert_eq!(
        try_read(local.as_ref(), Path::new("dir/drop.txt")).await.as_deref(),
        Some(&b"delete me from the server"[..]),
        "a DEFLATED entry in a folder extracts byte for byte"
    );
    assert_eq!(
        read_all(remote.as_ref(), &zip_path).await,
        two_entry_zip(),
        "an extract never touches the archive"
    );

    clean_deep(remote.as_ref(), &dir).await;
}

/// The parent-aware routing predicate sees a zip-INNER path on the server (the
/// `std::fs`-only one would say no), and the `.zip` itself stays a plain file.
pub(super) async fn a_zip_inner_path_on_the_server_routes_as_inside_the_archive(remote: Arc<dyn Volume>, dir: PathBuf) {
    use cmdr_fs::app_path::RelativeAppPath;

    let (_zip_path, seeded) = seed_zip_at(remote.as_ref(), &dir).await;
    let registered = Registered::new(&remote, "zip-routing");
    // A pane names the zip absolutely, under the volume's root; the fixtures spell
    // their scratch dir relative to it (`cmdr-test-…`), which no route can place.
    let zip = match seeded.to_canonical() {
        Ok(_) => seeded,
        Err(_) => remote
            .root()
            .join_relative(RelativeAppPath::new(seeded.as_str()).expect("a relative scratch path"))
            .expect("joins under the root"),
    };
    let inner = zip
        .join_relative(RelativeAppPath::new("dir/drop.txt").expect("a relative path"))
        .expect("joins under the zip");

    assert!(
        get_volume_manager()
            .path_is_inside_archive(&registered.id, &inner)
            .await,
        "a zip-inner path on the server must route to the archive-edit driver"
    );
    assert!(
        !get_volume_manager().path_is_inside_archive(&registered.id, &zip).await,
        "the `.zip` itself is a plain file, not archive-inner"
    );

    clean_deep(remote.as_ref(), &dir).await;
    registered.leave();
}

/// A remote EDIT (delete an entry) commits through pull → apply → upload → swap,
/// and re-reading the zip off the server shows it. No upload staging lingers.
pub(super) async fn a_remote_zip_edit_deletes_an_entry_on_the_server(remote: Arc<dyn Volume>, dir: PathBuf) {
    let zip_path = seed_zip(remote.as_ref(), &dir).await;

    let state = Arc::new(new_operation_state(app_engine(), Duration::from_millis(50)));
    let result = pull_apply_upload_swap(
        Arc::clone(&remote),
        "zip-edit",
        zip_path.clone(),
        state,
        None,
        |working: &Path| {
            let changeset = Changeset {
                deletes: vec!["dir/drop.txt".to_string()],
                ..Default::default()
            };
            mutator::apply(working, &changeset, &NoHooks).expect("local mutator apply");
            Ok::<(), EditError>(())
        },
    )
    .await;
    assert!(result.is_ok(), "a remote zip edit should commit");

    let back = read_server_zip(remote.as_ref(), &zip_path).await;
    assert_eq!(
        back.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["keep.txt"],
        "only the kept entry is left on the server"
    );
    assert_eq!(back.get("keep.txt").map(Vec::as_slice), Some(&b"keep me"[..]));
    assert_no_staging_litter(remote.as_ref(), &dir, "a remote zip edit").await;

    clean_deep(remote.as_ref(), &dir).await;
}

/// A cancel landing AFTER the local apply but BEFORE the swap leaves the
/// server's original byte for byte, and no upload staging behind.
pub(super) async fn a_cancel_before_the_swap_keeps_the_original_zip(remote: Arc<dyn Volume>, dir: PathBuf) {
    let zip_path = seed_zip(remote.as_ref(), &dir).await;

    let state = Arc::new(new_operation_state(app_engine(), Duration::from_millis(50)));
    let cancel = Arc::clone(&state);
    let result = pull_apply_upload_swap(
        Arc::clone(&remote),
        "zip-edit",
        zip_path.clone(),
        state,
        None,
        move |working: &Path| {
            let changeset = Changeset {
                deletes: vec!["dir/drop.txt".to_string()],
                ..Default::default()
            };
            mutator::apply(working, &changeset, &NoHooks).expect("local mutator apply");
            // `store` is the test's own lever: the orchestrator's pre-upload check
            // is what this cell is about, and no stop request path reaches it here.
            cancel.intent().set_for_test(OperationIntent::Stopped);
            Ok::<(), EditError>(())
        },
    )
    .await;
    assert!(
        matches!(result, Err(EditError::Cancelled)),
        "a cancel before the swap reports Cancelled"
    );

    assert_eq!(
        read_all(remote.as_ref(), &zip_path).await,
        two_entry_zip(),
        "❗ the server's original zip is byte-for-byte what it was"
    );
    assert_no_staging_litter(remote.as_ref(), &dir, "a cancelled remote zip edit").await;

    clean_deep(remote.as_ref(), &dir).await;
}

/// Local files COPIED INTO a zip on the server: the new entries join the old
/// ones, and the result re-reads off the server as a valid archive.
pub(super) async fn local_files_copied_into_a_zip_on_the_server_join_it(remote: Arc<dyn Volume>, dir: PathBuf) {
    let zip_path = seed_zip(remote.as_ref(), &dir).await;
    let registered = Registered::new(&remote, "zip-copy-into");
    let (_local_dir, local) = local_volume("zip-copy-into");
    seed(
        local.as_ref(),
        Path::new(""),
        &[("new.txt", b"fresh"), ("folder/deep.txt", b"deep")],
    )
    .await;

    let events = Arc::new(CollectorEventSink::new());
    app_engine()
        .route_archive_copy_into(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            local,
            vec![PathBuf::from("new.txt"), PathBuf::from("folder")],
            zip_path.clone(),
            registered.id.clone(),
            ConflictResolution::Stop,
            0,
            false,
            None,
            None,
        )
        .await
        .expect("start the copy into the remote zip");
    await_archive_op(&events, "the copy into the remote zip").await;

    let back = read_server_zip(remote.as_ref(), &zip_path).await;
    assert_eq!(back.get("keep.txt").map(Vec::as_slice), Some(&b"keep me"[..]));
    assert_eq!(
        back.get("dir/drop.txt").map(Vec::as_slice),
        Some(&b"delete me from the server"[..]),
        "the entries the zip already had survive"
    );
    assert_eq!(back.get("new.txt").map(Vec::as_slice), Some(&b"fresh"[..]));
    assert_eq!(
        back.get("folder/deep.txt").map(Vec::as_slice),
        Some(&b"deep"[..]),
        "a folder lands with its structure"
    );
    assert_no_staging_litter(remote.as_ref(), &dir, "a copy into a remote zip").await;

    clean_deep(remote.as_ref(), &dir).await;
    registered.leave();
}

/// Runs one compress of `sources` (on `source`) into `dest_zip` on the server.
async fn compress_onto(
    source: &Arc<dyn Volume>,
    sources: &[&str],
    dest_zip: PathBuf,
    parent_id: &str,
) -> Arc<CollectorEventSink> {
    let events = Arc::new(CollectorEventSink::new());
    cmdr_operations::testing::compress_start(
        app_engine(),
        Arc::clone(&events) as Arc<dyn OperationEventSink>,
        Arc::clone(source),
        sources.iter().map(PathBuf::from).collect(),
        dest_zip,
        parent_id.to_string(),
        ConflictResolution::Overwrite,
        100,
        None,
        None,
        Initiator::User,
    )
    .await
    .expect("start the compress");
    await_archive_op(&events, "the compress onto the server").await;
    events
}

/// Deterministic high-entropy bytes, so deflate cannot shrink this fixture below
/// the producer's four × 128 KiB output queue.
fn incompressible_bytes(len: usize, mut state: u64) -> Vec<u8> {
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// COMPRESS onto the server: local files packed into a NEW zip that lands on the
/// server through the seed-through-volume path, parses, holds the sources, and
/// leaves no upload staging.
pub(super) async fn a_compress_onto_the_server_lands_a_valid_zip(
    remote: Arc<dyn Volume>,
    dir: PathBuf,
    expect_direct: bool,
) {
    let registered = Registered::new(&remote, "compress");
    let (_local_dir, local) = local_volume("compress");
    let first = incompressible_bytes(700_003, 0x0123_4567_89ab_cdef);
    let second = incompressible_bytes(300_007, 0xfedc_ba98_7654_3210);
    seed(
        local.as_ref(),
        Path::new(""),
        &[("one.bin", &first), ("two.bin", &second)],
    )
    .await;

    let events = compress_onto(&local, &["one.bin", "two.bin"], dir.join("bundle.zip"), &registered.id).await;

    let used_transfer_phase = events.progress.lock_ignore_poison().iter().any(|event| {
        matches!(
            event.phase,
            WriteOperationPhase::Transferring | WriteOperationPhase::FinishingTransfer
        )
    });
    assert_eq!(
        used_transfer_phase, !expect_direct,
        "a direct fresh ZIP has no completed-spool transfer phase"
    );

    let back = read_server_zip(remote.as_ref(), &dir.join("bundle.zip")).await;
    assert_eq!(back.get("one.bin").map(Vec::as_slice), Some(first.as_slice()));
    assert_eq!(back.get("two.bin").map(Vec::as_slice), Some(second.as_slice()));
    assert_no_staging_litter(remote.as_ref(), &dir, "a compress onto the server").await;

    clean_deep(remote.as_ref(), &dir).await;
    registered.leave();
}

/// COMPRESS files ON the server into a new zip on the SAME server, over the one
/// volume (so one connection): the direct pipeline reads the sources and writes
/// the stage concurrently, with output past every bounded queue, and must neither
/// deadlock nor mix the two streams up.
pub(super) async fn a_compress_of_server_files_onto_the_same_server_lands_a_valid_zip(
    remote: Arc<dyn Volume>,
    dir: PathBuf,
) {
    let registered = Registered::new(&remote, "compress-same-server");
    let first = incompressible_bytes(700_003, 0x1357_9bdf_2468_ace0);
    let second = incompressible_bytes(300_007, 0x0f1e_2d3c_4b5a_6978);
    let sources = dir.join("src");
    seed(remote.as_ref(), &sources, &[("one.bin", &first), ("two.bin", &second)]).await;
    let one = sources.join("one.bin");
    let two = sources.join("two.bin");
    let one = one.to_string_lossy();
    let two = two.to_string_lossy();

    let events = compress_onto(&remote, &[&one, &two], dir.join("bundle.zip"), &registered.id).await;

    assert!(
        !events.progress.lock_ignore_poison().iter().any(|event| {
            matches!(
                event.phase,
                WriteOperationPhase::Transferring | WriteOperationPhase::FinishingTransfer
            )
        }),
        "a same-server compress streams directly, with no spool upload"
    );
    let back = read_server_zip(remote.as_ref(), &dir.join("bundle.zip")).await;
    assert_eq!(back.get("one.bin").map(Vec::as_slice), Some(first.as_slice()));
    assert_eq!(back.get("two.bin").map(Vec::as_slice), Some(second.as_slice()));
    assert_no_staging_litter(remote.as_ref(), &dir, "a same-server compress").await;

    clean_deep(remote.as_ref(), &dir).await;
    registered.leave();
}

/// A compress onto a name the server holds in the other Unicode spelling: the
/// dialog's probe (`destination_exists`) says it's there, `path_exists` stays
/// byte-exact, and the compress replaces THAT archive in place, so the folder
/// ends with one entry in the server's spelling. A NEW archive's name then lands
/// composed.
pub(super) async fn a_compress_replaces_a_look_alike_archive_in_place(remote: Arc<dyn Volume>, dir: PathBuf) {
    use crate::commands::file_system::{destination_exists, path_exists};

    let zip_nfc = CAFE_NFC.replace(".txt", ".zip");
    let zip_nfd = CAFE_NFD.replace(".txt", ".zip");
    seed(remote.as_ref(), &dir, &[(&zip_nfc, b"THEIR ARCHIVE")]).await;
    let registered = Registered::new(&remote, "compress-look-alike");

    let asked = dir.join(&zip_nfd).display().to_string();
    let there = destination_exists(Some(registered.id.clone()), asked.clone()).await;
    assert!(there.data && !there.timed_out, "the dialog must warn: {there:?}");
    let exact = path_exists(Some(registered.id.clone()), asked).await;
    assert!(
        !exact.data && !exact.timed_out,
        "path_exists stays byte-exact: {exact:?}"
    );

    let (_local_dir, local) = local_volume("compress-look-alike");
    seed(local.as_ref(), Path::new(""), &[("one.txt", b"first")]).await;
    compress_onto(&local, &["one.txt"], dir.join(&zip_nfd), &registered.id).await;

    assert_eq!(
        names_in(remote.as_ref(), &dir).await,
        vec![zip_nfc.clone()],
        "❗ one archive, in the server's spelling"
    );
    let back = read_server_zip(remote.as_ref(), &dir.join(&zip_nfc)).await;
    assert_eq!(
        back.get("one.txt").map(Vec::as_slice),
        Some(&b"first"[..]),
        "the archive the server held is replaced by the new zip in place"
    );

    let resume_nfd = RESUME_NFD.replace(".txt", ".zip");
    let new_there = destination_exists(Some(registered.id.clone()), dir.join(&resume_nfd).display().to_string()).await;
    assert!(
        !new_there.data && !new_there.timed_out,
        "a free name reads free: {new_there:?}"
    );
    compress_onto(&local, &["one.txt"], dir.join(&resume_nfd), &registered.id).await;
    assert_composes_new_names(remote.as_ref());
    let mut expected = vec![zip_nfc.clone(), RESUME_NFC.replace(".txt", ".zip")];
    expected.sort();
    assert_eq!(
        names_in(remote.as_ref(), &dir).await,
        expected,
        "❗ a new archive lands beside it, composed"
    );

    clean_deep(remote.as_ref(), &dir).await;
    registered.leave();
}
