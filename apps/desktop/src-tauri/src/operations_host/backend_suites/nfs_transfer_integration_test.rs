//! Real copies between a local disk and the knfsd fixture's `/insecure` export,
//! driven through the app's own `copy_between_volumes`.
//!
//! ❗ **The point is the entry point**, as for the SFTP and WebDAV twins: the
//! crate's own cells exercise every method the copy engine calls, and a copy
//! can still be broken in the app by a capability predicate or a pre-flight the
//! crate never sees. So these start where the transfer dialog starts, and every
//! cell checksums the bytes at BOTH ends.
//!
//! The scenarios live in the `network_*_test_support.rs` files, shared with
//! every server backend; the cells stay here because the integration lane
//! selects them by the `nfs_integration_` name prefix.

use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::volume::Volume;
use cmdr_operations::VolumeCopyConfig;

use super::network_dates_test_support::{
    a_copy_off_the_server_keeps_the_source_date, a_copy_onto_the_server_keeps_the_source_date,
    copied_folders_off_the_server_keep_their_dates, copied_folders_onto_the_server_keep_their_dates,
};
use super::network_gated_source_test_support::{CANCEL_PAYLOAD_BYTES, gated_upload};
use super::network_transfer_test_support::{
    a_cancelled_upload_leaves_nothing_behind, a_directory_tree_lands_intact_off_the_server,
    a_directory_tree_lands_intact_on_the_server, a_pre_existing_destination_still_probes_each_name,
    an_overwrite_answer_replaces_the_destination_bytes, assert_no_staging_litter, awkward_names_survive_a_round_trip,
    budget, clean_deep, read_all, run_copy, self_describing_bytes, sha256, start_copy,
};
use super::nfs_test_support::fixture;
use crate::ignore_poison::IgnorePoison;
use crate::test_support::TestDir;
use cmdr_local::LocalPosixVolume;

/// Big enough to cross several WRITE and READ chunks rather than ride in one
/// call, so reassembly and offset bookkeeping are actually exercised.
const PAYLOAD_BYTES: usize = 700_000;

/// What the fixture's dial needs, said once for every `#[ignore]` below.
const FIXTURE: &str = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)";

fn local_volume(dir: &TestDir) -> Arc<dyn Volume> {
    Arc::new(LocalPosixVolume::local_folder(
        "Local",
        &**dir,
        crate::volume_host::host(),
    ))
}

/// Copy OFF the export: the direction `supports_export()` gates.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_copying_off_an_export_lands_every_byte() {
    let (remote, dir) = fixture("app-copy-from").await;
    let content = self_describing_bytes(PAYLOAD_BYTES, "downloaded.bin");
    let remote_file = dir.join("downloaded.bin");
    remote
        .create_file(&AppPath::legacy_unchecked(&remote_file), &content)
        .await
        .expect(FIXTURE);
    // The checksum at the SOURCE end, taken off the server itself, so a bad
    // seed can't make a bad copy look good.
    let source_digest = sha256(&read_all(remote.as_ref(), &remote_file).await);
    assert_eq!(source_digest, sha256(&content), "the fixture seed must round-trip");
    let local_dir = TestDir::new("nfs_copy_off_export");
    let local = local_volume(&local_dir);

    run_copy(
        "copy-off-nfs",
        Arc::clone(&remote),
        vec![remote_file],
        Arc::clone(&local),
        PathBuf::from(""),
    )
    .await;

    let landed = read_all(local.as_ref(), Path::new("downloaded.bin")).await;
    assert_eq!(
        sha256(&landed),
        source_digest,
        "local disk must hold what the export holds"
    );
    clean_deep(remote.as_ref(), &dir).await;
}

/// Copy ONTO the export: `UNSTABLE` WRITEs, their COMMIT, the staging temp,
/// and the landing rename, all through the app.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_copying_onto_an_export_lands_every_byte() {
    let (remote, dir) = fixture("app-copy-onto").await;
    let content = self_describing_bytes(PAYLOAD_BYTES, "uploaded.bin");
    let local_dir = TestDir::new("nfs_copy_onto_export");
    let local = local_volume(&local_dir);
    local
        .create_file(&AppPath::legacy_unchecked(Path::new("uploaded.bin")), &content)
        .await
        .expect("seed the local file");

    run_copy(
        "copy-onto-nfs",
        Arc::clone(&local),
        vec![PathBuf::from("uploaded.bin")],
        Arc::clone(&remote),
        dir.clone(),
    )
    .await;

    let landed = read_all(remote.as_ref(), &dir.join("uploaded.bin")).await;
    assert_eq!(
        sha256(&landed),
        sha256(&content),
        "the export must hold what local disk holds"
    );
    assert_no_staging_litter(remote.as_ref(), &dir, "a plain upload").await;
    clean_deep(remote.as_ref(), &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_copying_a_directory_tree_onto_an_export_lands_it_intact() {
    let (remote, dir) = fixture("app-tree-onto").await;
    a_directory_tree_lands_intact_on_the_server(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_copying_a_directory_tree_off_an_export_lands_it_intact() {
    let (remote, dir) = fixture("app-tree-off").await;
    a_directory_tree_lands_intact_off_the_server(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_a_cancelled_upload_leaves_nothing_behind() {
    let (remote, dir) = fixture("app-cancelled-upload").await;
    a_cancelled_upload_leaves_nothing_behind(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_an_overwrite_answer_replaces_the_destination_bytes() {
    let (remote, dir) = fixture("app-overwrite").await;
    an_overwrite_answer_replaces_the_destination_bytes(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_a_pre_existing_destination_still_probes_each_name() {
    let (remote, dir) = fixture("app-pre-existing").await;
    a_pre_existing_destination_still_probes_each_name(remote, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_awkward_names_survive_a_round_trip() {
    let (remote, dir) = fixture("app-awkward-names").await;
    awkward_names_survive_a_round_trip(remote, dir).await;
}

/// A paused upload holds still, resumes, and lands: NFS keeps no request open
/// across the pause (each WRITE is its own call), so there's no server
/// patience to outlast, only the engine's own pause to honor.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_a_paused_upload_resumes_and_lands() {
    let (remote, dir) = fixture("app-paused-upload").await;
    let bytes = self_describing_bytes(CANCEL_PAYLOAD_BYTES, "paused");
    let source = gated_upload(bytes.clone()).await;
    let running = start_copy(
        "nfs-pause",
        Arc::clone(&source.volume),
        vec![PathBuf::from("/big.bin")],
        Arc::clone(&remote),
        dir.clone(),
        VolumeCopyConfig::default(),
    )
    .await;
    source.gate.add_permits(2);
    crate::test_support::wait_until_async(budget(Duration::from_secs(6)), "two chunks into the upload", || {
        source.handed_out.load(std::sync::atomic::Ordering::SeqCst) >= 2
    })
    .await;
    assert!(
        cmdr_operations::testing::pause_write_operation(&running.engine, &running.operation_id),
        "the upload is parked on the gate, so it's still running to pause"
    );
    source.gate.add_permits(100_000);
    // allowed-test-sleep: a pause is the absence of progress, and only time can show nothing moved.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        running.events.complete.lock_ignore_poison().is_empty(),
        "a paused upload doesn't finish"
    );
    assert!(
        !remote.exists(&AppPath::legacy_unchecked(&dir.join("big.bin"))).await,
        "nothing wears the user's name mid-pause"
    );

    assert!(cmdr_operations::testing::resume_write_operation(
        &running.engine,
        &running.operation_id
    ));
    running.settle().await;
    running.assert_no_errors();

    assert_eq!(
        sha256(&read_all(remote.as_ref(), &dir.join("big.bin")).await),
        sha256(&bytes),
        "every byte lands once the pause lifts"
    );
    assert_no_staging_litter(remote.as_ref(), &dir, "a paused upload").await;
    clean_deep(remote.as_ref(), &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_a_copy_onto_an_export_keeps_the_source_date() {
    let (remote, dir) = fixture("app-dated-onto").await;
    a_copy_onto_the_server_keeps_the_source_date(remote, dir, Duration::ZERO).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_a_copy_off_an_export_keeps_the_source_date() {
    let (remote, dir) = fixture("app-dated-off").await;
    a_copy_off_the_server_keeps_the_source_date(remote, dir, Duration::ZERO).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_folders_copied_onto_an_export_keep_their_dates() {
    let (remote, dir) = fixture("app-dated-folders-onto").await;
    copied_folders_onto_the_server_keep_their_dates(remote, dir, Duration::ZERO).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_folders_copied_off_an_export_keep_their_dates() {
    let (remote, dir) = fixture("app-dated-folders-off").await;
    copied_folders_off_the_server_keep_their_dates(remote, dir).await;
}

/// A file's mode survives the trip onto the export: it rides the SETATTR the
/// date already costs, folded under the user's umask the way a routed copy
/// folds it (`0640` stays `0640` under any common umask).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_a_copy_onto_an_export_keeps_the_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let (remote, dir) = fixture("app-mode-onto").await;
    let local_dir = TestDir::new("nfs_mode_onto_export");
    let file = local_dir.join("private.txt");
    std::fs::write(&file, b"owner and group only").expect("seed the local file");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).expect("mode it");

    run_copy(
        "mode-onto-nfs",
        local_volume(&local_dir),
        vec![PathBuf::from("private.txt")],
        Arc::clone(&remote),
        dir.clone(),
    )
    .await;

    let landed = remote
        .get_metadata(&AppPath::legacy_unchecked(&dir.join("private.txt")))
        .await
        .expect(FIXTURE);
    assert_eq!(
        landed.permissions & 0o777,
        0o640,
        "the source's mode, folded, on the export"
    );
    clean_deep(remote.as_ref(), &dir).await;
}

/// The loss note against a real export: a tagged local file copied onto NFS
/// (which keeps no xattrs, and writes no `._` sidecar) reports its tags on the
/// completion, and an untagged one reports nothing. Both land every byte.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_a_tagged_upload_reports_its_tags_and_an_untagged_one_nothing() {
    use cmdr_operations::testing::{LostMetadata, LostMetadataClass, MetadataLeftBehind};

    let (remote, dir) = fixture("app-metadata-loss").await;
    let local_dir = TestDir::new("nfs_metadata_loss");
    for name in ["tagged.txt", "plain.txt"] {
        let file = std::fs::File::create(local_dir.join(name)).expect("seed the local file");
        std::io::Write::write_all(&mut &file, name.as_bytes()).expect("write it");
        // An mtime before its birth drags APFS's birth down to it (and Linux's
        // birth is after it), so neither has a creation date worth reporting:
        // the tags are the only difference between the two.
        file.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_611_909_015))
            .expect("date it");
    }
    xattr::set(local_dir.join("tagged.txt"), "user.cmdr.tag", b"red").expect("tag it");
    let local = local_volume(&local_dir);
    let left_behind = |copy: &super::network_transfer_test_support::RunningCopy| {
        copy.events.complete.lock_ignore_poison()[0]
            .metadata_left_behind
            .clone()
    };

    let tagged = run_copy(
        "metadata-loss-tagged",
        Arc::clone(&local),
        vec![PathBuf::from("tagged.txt")],
        Arc::clone(&remote),
        dir.clone(),
    )
    .await;
    let plain = run_copy(
        "metadata-loss-plain",
        local,
        vec![PathBuf::from("plain.txt")],
        Arc::clone(&remote),
        dir.clone(),
    )
    .await;

    assert_eq!(
        left_behind(&tagged),
        Some(MetadataLeftBehind {
            classes: vec![LostMetadata {
                class: LostMetadataClass::ExtendedAttributes,
                item_count: 1
            }]
        }),
        "the tags stayed behind, and the note says so"
    );
    assert_eq!(left_behind(&plain), None, "a plain file loses nothing worth a note");
    assert_eq!(read_all(remote.as_ref(), &dir.join("tagged.txt")).await, b"tagged.txt");
    assert!(
        !remote
            .exists(&AppPath::legacy_unchecked(&dir.join("._tagged.txt")))
            .await,
        "❌ no AppleDouble sidecar"
    );
    clean_deep(remote.as_ref(), &dir).await;
}
