//! The saved share behind a mount (`saved_share_of_mount`): how a mount finds the server
//! and account the person saved, whatever address macOS reports it by.

use super::*;

/// A share row the way a mount through Cmdr files it, its last mount at `/Volumes/<share>`.
fn row(server_name: &str, address: &str, share: &str, username: Option<&str>, volume_id: &str) -> KnownNetworkShare {
    KnownNetworkShare {
        server_name: server_name.to_string(),
        share_name: share.to_string(),
        protocol: "smb".to_string(),
        last_connected_at: "2026-11-26T20:00:00Z".to_string(),
        last_connection_mode: if username.is_some() {
            ConnectionMode::Credentials
        } else {
            ConnectionMode::Guest
        },
        last_known_auth_options: AuthOptions::GuestOrCredentials,
        username: username.map(str::to_string),
        address: Some(address.to_string()),
        port: None,
        volume_id: Some(volume_id.to_string()),
        mount_path: Some(format!("/Volumes/{share}")),
        pinned: true,
        server_guid: None,
        server_guid_proof: GuidProof::Unauthenticated,
    }
}

/// ❗ A mount Cmdr made by the server's NAME can come back spelled by the address macOS
/// resolved it to (an IPv6 ULA), which no saved server is called. The row the mount filed
/// knows both: QA 2026-11-26 had `..` make a new IPv6 server asking for a sign-in, and the
/// upgrade go out as a guest, beside the saved server and account.
#[test]
fn a_mount_spelled_by_its_ipv6_address_finds_the_saved_share_it_filed() {
    let by_ip = row(
        "198.51.100.150",
        "198.51.100.150",
        "media",
        None,
        "smb-198-51-100-150-445-media",
    );
    let by_name = row(
        "testnas",
        "fd00::1",
        "media",
        Some("test_user_1"),
        &cmdr_fs::volume::smb_volume_id("fd00::1", 445, "media"),
    );
    let rows = [by_ip, by_name];

    let found = saved_share_of_mount_in(&rows, "fd00::1", 445, "media", "/Volumes/media-1");
    assert_eq!(found.map(|row| row.server_name.as_str()), Some("testnas"));
    assert_eq!(
        found.and_then(|row| row.username.as_deref()),
        Some("test_user_1"),
        "the account it opens as"
    );
}

/// The id moves when macOS spells the server another way on the next mount; the path the
/// row's last mount sat at still names it, for the same share.
#[test]
fn a_mount_under_a_new_id_finds_its_saved_share_by_where_it_sits() {
    let mut saved = row("testnas", "fd00::1", "media", Some("test_user_1"), "smb-old-spelling");
    saved.mount_path = Some("/Volumes/media-1".to_string());
    let rows = [saved];

    let found = saved_share_of_mount_in(&rows, "fd00::2", 445, "Media", "/Volumes/media-1");
    assert_eq!(found.map(|row| row.server_name.as_str()), Some("testnas"));
    assert!(
        saved_share_of_mount_in(&rows, "fd00::2", 445, "other", "/Volumes/media-1").is_none(),
        "another share at that path isn't this row's"
    );
    assert!(saved_share_of_mount_in(&rows, "fd00::2", 445, "media", "/Volumes/media").is_none());
}

/// The sign-in history row (no share) is never a mount's saved share.
#[test]
fn a_mount_nothing_saved_has_no_saved_share() {
    let mut history = row("testnas", "testnas", "", Some("test_user_1"), "unused");
    history.volume_id = None;
    history.mount_path = None;
    assert!(saved_share_of_mount_in(&[history], "testnas", 445, "", "/Volumes/media").is_none());
}
