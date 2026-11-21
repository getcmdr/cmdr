//! The kernel-mount → direct-connection upgrade, with a pane open in an
//! accented folder, against the Docker SMB fixture.
//!
//! The kernel mount decomposes every name it hands out, so a pane that listed
//! `fotók` through it holds `fotók` decomposed, and so does every entry path in
//! that listing. The direct connection matches names byte-for-byte, and the
//! fixture's Samba stores names as sent, the way David's QNAP does: without the
//! respell the swap leaves a pane whose every accented entry opens nowhere.

#![cfg(unix)] // POSIX-only cells: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan, task 35 and #1225).

use super::*;
use crate::operations_host::backend_suites::smb_test_support::*;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_listing::testing::TestListing;
use cmdr_smb::volume::{MountAnchor, connect_smb_volume};
use unicode_normalization::UnicodeNormalization;

/// `fotók`, composed: how the share stores the folder.
const NFC_DIR: &str = "fot\u{f3}k";
/// `retusált.jpg`, decomposed: how the share stores the photo inside it.
const NFD_FILE: &str = "retusa\u{301}lt.jpg";
const PAYLOAD: &[u8] = b"a photo the upgrade must not strand";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_pane_in_an_accented_folder_survives_the_upgrade_to_a_direct_connection() {
    use crate::test_support::wait_until_async;

    let seeder = make_docker_volume().await;
    let volume_id = seeder.volume_id().to_string();
    let top = test_dir_name();
    ensure_clean(&seeder, &top).await;
    let album = share_path(&format!("{top}/{NFC_DIR}"));
    seeder
        .create_directory(AppPath::from_normalized(&share_path(&top)).expect("a canonical app path"))
        .await
        .unwrap();
    seeder
        .create_directory(AppPath::from_normalized(&album).expect("a canonical app path"))
        .await
        .unwrap();
    let photo = format!("{album}/{NFD_FILE}");
    seeder
        .create_file(AppPath::from_normalized(&photo).expect("a canonical app path"), PAYLOAD)
        .await
        .unwrap();

    // What the kernel mount handed the pane: the folder and its photo, decomposed.
    let kernel_album: String = album.nfd().collect();
    let kernel_photo = format!("{kernel_album}/{NFD_FILE}");
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(&kernel_album)
        .entries(vec![FileEntry::new(NFD_FILE.to_string(), kernel_photo, false, false)])
        .insert(crate::listing_host::listings(), "upgrade-respell");

    let successor = connect_smb_volume(
        "public",
        MountAnchor::at_share_root(TEST_MOUNT_ROOT),
        &volume_id,
        docker_guest_params(),
        crate::volume_host::host(),
    )
    .await
    .expect("the direct connection to the Docker SMB container");
    register_replacing_predecessor(&volume_id, Arc::new(successor)).await;

    let respelled = || {
        listing
            .engine()
            .location(&listing.listing_id())
            .is_some_and(|(_, path)| path == Path::new(&album))
    };
    wait_until_async(Duration::from_secs(5), "the open listing to be respelled", respelled).await;
    let entry_paths: Vec<String> = listing.entries().into_iter().map(|e| e.path).collect();
    let direct = crate::file_system::volume::manager::get_volume_manager()
        .get(&volume_id)
        .expect("the direct volume is registered");
    let read = async {
        let mut stream = direct
            .open_read_stream(AppPath::from_normalized(&entry_paths[0]).expect("a canonical app path"))
            .await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next_chunk().await {
            bytes.extend_from_slice(&chunk?);
        }
        Ok::<_, VolumeError>(bytes)
    }
    .await;

    crate::file_system::volume::manager::get_volume_manager().unregister(&volume_id);
    seeder
        .delete(AppPath::from_normalized(&photo).expect("a canonical app path"))
        .await
        .ok();
    ensure_clean(&seeder, &top).await;

    assert_eq!(entry_paths, vec![photo.clone()], "the entry carries the server's bytes");
    assert_eq!(
        read.expect("the photo in the open pane must open through the direct connection"),
        PAYLOAD
    );
}
