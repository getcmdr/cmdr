//! A folder link that leads back up its own path, on a real Samba share.
//!
//! Samba lists a Unix symlink as the folder it points to (no reparse bit:
//! `links_integration_test::smb_integration_samba_lists_a_symlink_as_its_target`),
//! so a walk can't tell `loop -> ..` from a folder by its listing, and descending
//! it reaches the same folder again, one level deeper each pass. These cells pin
//! that a walk stops at it anyway.
//!
//! `#[ignore]`d like every Docker cell: start `apps/desktop/test/smb-servers/start.sh`,
//! then `cargo nextest run -E 'package(cmdr-smb)' --run-ignored only`.

use super::test_support::*;
use super::*;

/// `<dir>/a/f.txt` written through the share, then `<dir>/a/loop -> ..` made on
/// the server's disk: from inside `a`, the link reaches `<dir>`, which holds `a`.
async fn seed_loop(vol: &SmbVolume, dir: &str) {
    vol.create_directory(&app_at(vol.root(), dir)).await.unwrap();
    vol.create_directory(&app_at(vol.root(), &format!("{dir}/a")))
        .await
        .unwrap();
    vol.create_file(&app_at(vol.root(), &format!("{dir}/a/f.txt")), b"four")
        .await
        .unwrap();
    run_in_the_guest_container(&["ln", "-s", "..", &format!("/shares/public/{dir}/a/loop")]);
}

/// Takes the link out on the server's disk, so the cleanup's delete walk never
/// meets it.
fn unseed_loop(dir: &str) {
    run_in_the_guest_container(&["rm", "-f", &format!("/shares/public/{dir}/a/loop")]);
}

/// "Calculate size" on a folder holding a link back up its own path counts the
/// folder once: one file, four bytes. Following the link counted the same file
/// once per pass until the server's link limit failed the scan.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_size_count_stops_at_a_link_back_up_its_own_path() {
    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    ensure_clean(&vol, &dir).await;
    seed_loop(&vol, &dir).await;

    let scanned = tokio::time::timeout(Duration::from_secs(15), vol.scan_for_copy(&app_at(vol.root(), &dir)))
        .await
        .expect("the scan ends");
    unseed_loop(&dir);
    ensure_clean(&vol, &dir).await;

    let scanned = scanned.expect("the scan succeeds");
    assert_eq!(
        (scanned.file_count, scanned.total_bytes),
        (1, 4),
        "the file behind the loop counted once"
    );
}
