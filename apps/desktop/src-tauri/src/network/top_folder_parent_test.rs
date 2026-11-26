//! Where `..` leads out of a server's top folder, read off the Servers list's rows.

use std::collections::HashMap;

use cmdr_fs::server_meta::ServerProtocol;

use super::{ParentRowSwitch, TopFolderParent, place_parent, share_list_parent, share_top_parent};
use crate::network::servers_listing::{ServerRow, ServerRowInputs, server_rows};
use crate::network::smb_share_parent::ShareParent;
use crate::network::{HostSource, NetworkHost};
use crate::saved_servers::{SavedPlace, SavedServer, ServerNameSource};

const ACCOUNT: &str = "s3-s3-example-com-443-test-key-1-root";
const ACCOUNT_ROOT: &str = "s3://test_key_1@s3.example.com:443/";
const BUCKET: &str = "s3-s3-example-com-443-test-key-1-bucket-1";
const BUCKET_ROOT: &str = "s3://test_key_1@s3.example.com:443/bucket_1";
const SFTP: &str = "sftp-testnas.local-22-test_user_1";
const SFTP_ROOT: &str = "sftp://test_user_1@testnas.local:22/srv";

fn saved_place(volume_id: &str, app_root: &str) -> SavedPlace {
    SavedPlace {
        volume_id: volume_id.to_string(),
        name: volume_id.to_string(),
        pinned: true,
        connected: true,
        app_root: app_root.to_string(),
        username: Some("test_user_1".to_string()),
        auto_reconnect: Some(true),
    }
}

fn server(id: &str, protocol: ServerProtocol, places: Vec<SavedPlace>) -> SavedServer {
    SavedServer {
        id: id.to_string(),
        protocol,
        display_name: id.to_string(),
        name_source: ServerNameSource::User,
        address: "testnas.local".to_string(),
        username: Some("test_user_1".to_string()),
        pinned: false,
        last_connected_at: None,
        auto_reconnect: Some(true),
        places,
        awaiting_first_connect: false,
        saved_from_mount: false,
    }
}

fn rows(saved: &[SavedServer]) -> Vec<ServerRow> {
    server_rows(&ServerRowInputs {
        saved,
        hosts: &[],
        volume_states: &HashMap::new(),
        smb_mounts: &[],
        listed_as: &HashMap::new(),
    })
}

fn servers_list_on(row_path: &str) -> TopFolderParent {
    TopFolderParent {
        volume_id: "network".to_string(),
        path: "smb://".to_string(),
        select_path: Some(row_path.to_string()),
        switch: ParentRowSwitch::Server,
    }
}

#[test]
fn a_one_place_servers_top_folder_leads_to_its_row_on_the_servers_list() {
    let rows = rows(&[server(SFTP, ServerProtocol::Sftp, vec![saved_place(SFTP, SFTP_ROOT)])]);
    assert_eq!(place_parent(&rows, SFTP, SFTP_ROOT), Some(servers_list_on(SFTP_ROOT)));
}

#[test]
fn an_s3_accounts_root_leads_to_its_row_on_the_servers_list() {
    let rows = rows(&[server(
        ACCOUNT,
        ServerProtocol::S3,
        vec![saved_place(BUCKET, BUCKET_ROOT), saved_place(ACCOUNT, ACCOUNT_ROOT)],
    )]);
    assert_eq!(
        place_parent(&rows, ACCOUNT, ACCOUNT_ROOT),
        Some(servers_list_on(ACCOUNT_ROOT))
    );
}

/// A bucket opened as its own place (David's QA, 2026-11-26: no `..` in its top folder)
/// leads to the bucket list it sits in, on the bucket.
#[test]
fn a_buckets_top_folder_leads_to_its_accounts_bucket_list_on_the_bucket() {
    let rows = rows(&[server(
        ACCOUNT,
        ServerProtocol::S3,
        vec![saved_place(BUCKET, BUCKET_ROOT), saved_place(ACCOUNT, ACCOUNT_ROOT)],
    )]);
    assert_eq!(
        place_parent(&rows, BUCKET, BUCKET_ROOT),
        Some(TopFolderParent {
            volume_id: ACCOUNT.to_string(),
            path: ACCOUNT_ROOT.to_string(),
            select_path: Some(BUCKET_ROOT.to_string()),
            switch: ParentRowSwitch::Server,
        })
    );
}

/// With only the bucket saved there's no bucket list to land in yet, so `..` goes to the
/// account's row, where Enter saves and opens the root.
#[test]
fn a_bucket_of_an_account_whose_root_isnt_saved_leads_to_the_accounts_row() {
    let rows = rows(&[server(
        ACCOUNT,
        ServerProtocol::S3,
        vec![saved_place(BUCKET, BUCKET_ROOT)],
    )]);
    assert_eq!(
        place_parent(&rows, BUCKET, BUCKET_ROOT),
        Some(servers_list_on(ACCOUNT_ROOT))
    );
}

#[test]
fn a_volume_no_server_holds_has_no_way_up() {
    let rows = rows(&[server(SFTP, ServerProtocol::Sftp, vec![saved_place(SFTP, SFTP_ROOT)])]);
    assert_eq!(place_parent(&rows, "root", "/"), None);
}

/// An SMB server's saved shares are no places here: a share's `..` comes from its mount.
#[test]
fn an_smb_servers_saved_share_is_no_place_with_a_way_up() {
    let rows = rows(&[server(
        "manual-testnas-445",
        ServerProtocol::Smb,
        vec![saved_place("smb-testnas-media", "/Volumes/media")],
    )]);
    assert_eq!(place_parent(&rows, "smb-testnas-media", "/Volumes/media"), None);
}

#[test]
fn a_share_list_leads_to_the_servers_list_on_its_servers_row() {
    let parent = share_list_parent("smb://testnas/", Some("smb://192.168.1.111".to_string()));
    assert_eq!(parent.path, "smb://");
    assert_eq!(parent.select_path.as_deref(), Some("smb://192.168.1.111"));
    assert_eq!(parent.switch, ParentRowSwitch::Always);
    // A server no row stands for: the cursor goes by the share list's own path.
    let unlisted = share_list_parent("smb://testnas/", None);
    assert_eq!(unlisted.select_path.as_deref(), Some("smb://testnas"));
}

#[test]
fn a_shares_top_folder_leads_to_its_servers_share_list() {
    let share = ShareParent {
        host: NetworkHost {
            id: "testnas".to_string(),
            name: "Testnas".to_string(),
            hostname: Some("testnas.local".to_string()),
            ip_address: None,
            port: 445,
            source: HostSource::Discovered,
        },
        server_label: "Testnas".to_string(),
        share_name: "media".to_string(),
        show_parent_row: None,
    };
    let parent = share_top_parent(&share);
    assert_eq!(parent.volume_id, "network");
    assert_eq!(parent.path, "smb://testnas");
    assert_eq!(parent.switch, ParentRowSwitch::Share);
}
