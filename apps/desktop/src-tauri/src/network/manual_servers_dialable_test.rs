//! Tests for `manual_servers_dialable.rs`: a saved SMB entry always names an address a dial
//! can reach, and an entry written under a Bonjour display name never gets dialed.

use super::super::account::set_account_at_path;
use super::super::{MANUAL_SERVERS_FILENAME, ManualServerEntry, generate_server_id, read_store_from_path};
use super::*;
use crate::network::HostSource;

fn bonjour(name: &str, hostname: Option<&str>, ip: Option<&str>, port: u16) -> NetworkHost {
    NetworkHost {
        id: format!("bonjour-{port}"),
        name: name.to_string(),
        hostname: hostname.map(str::to_string),
        ip_address: ip.map(str::to_string),
        port,
        source: HostSource::Discovered,
    }
}

fn entry(address: &str, port: u16, username: Option<&str>) -> ManualServerEntry {
    ManualServerEntry {
        id: generate_server_id(address, port),
        display_name: String::new(),
        address: address.to_string(),
        port,
        added_at: "2026-01-01T00:00:00Z".to_string(),
        username: username.map(str::to_string),
        awaiting_first_connect: false,
        saved_from_mount: false,
    }
}

fn addresses(entries: &[ManualServerEntry]) -> Vec<(String, u16, Option<String>)> {
    entries
        .iter()
        .map(|e| (e.address.clone(), e.port, e.username.clone()))
        .collect()
}

#[test]
fn a_hostname_or_an_ip_is_dialable_and_a_display_name_is_not() {
    for address in [
        "testnas.local",
        "10.0.0.4",
        "my_nas",
        "fe80::1",
        "[fe80::1]",
        "localhost",
    ] {
        assert!(is_dialable(address), "{address}");
    }
    for address in ["SMB Test (Auth)", "My NAS", "", "  "] {
        assert!(!is_dialable(address), "{address:?}");
    }
}

/// ❗ A discovered host is dialed at its hostname (else its IP) on ITS port, ❌ never at its
/// Bonjour display name on 445: "Sign in as…" once saved `SMB Test (Auth)` as an address.
#[test]
fn a_discovered_host_is_saved_at_its_hostname_and_port() {
    let host = bonjour("SMB Test (Auth)", Some("smb-auth"), None, 11482);
    assert_eq!(dialable_server_of(&host), Some(SmbServer::new("smb-auth", 11482)));

    let by_ip = bonjour("Testnas", None, Some("10.0.0.4"), 445);
    assert_eq!(dialable_server_of(&by_ip), Some(SmbServer::new("10.0.0.4", 445)));

    let unresolved = bonjour("Testnas", None, None, 445);
    assert_eq!(
        dialable_server_of(&unresolved),
        None,
        "not resolved yet: nothing to save"
    );

    let named_as_host = bonjour("SMB Test (Auth)", Some("SMB Test (Auth)"), None, 445);
    assert_eq!(dialable_server_of(&named_as_host), None);
}

/// A host known only by name is looked up under it when the name is an address.
#[test]
fn a_name_is_an_address_only_when_it_spells_one() {
    assert_eq!(
        dialable_name("localhost:11482"),
        Some(SmbServer::new("localhost", 11482))
    );
    assert_eq!(dialable_name("10.0.0.4"), Some(SmbServer::new("10.0.0.4", 445)));
    assert_eq!(dialable_name("SMB Test (Auth)"), None);
}

#[test]
fn the_store_never_saves_an_account_under_a_display_name() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(MANUAL_SERVERS_FILENAME);
    assert!(set_account_at_path(&path, &SmbServer::new("SMB Test (Auth)", 445), &[], Some("test_user_1")).is_none());
    assert!(read_store_from_path(&path).servers.is_empty());
}

/// ❗ An entry written under a display name is repaired onto the discovered host it names
/// (by identity), keeping the account, so the next listing signs in as it.
#[test]
fn a_display_name_entry_becomes_the_discovered_host_it_names() {
    let hosts = [bonjour("SMB Test (Auth)", Some("smb-auth"), None, 11482)];
    let usable = usable_entries(
        vec![
            entry("SMB Test (Auth)", 445, Some("test_user_1")),
            entry("10.0.0.9", 445, None),
        ],
        &hosts,
    );
    assert_eq!(
        addresses(&usable),
        vec![
            ("10.0.0.9".to_string(), 445, None),
            ("smb-auth".to_string(), 11482, Some("test_user_1".to_string())),
        ]
    );
    assert_eq!(usable[1].id, generate_server_id("smb-auth", 11482));
}

/// With no discovered host to pair it with, the entry is skipped: never dialed, never listed.
#[test]
fn a_display_name_entry_nothing_names_is_skipped() {
    let usable = usable_entries(vec![entry("SMB Test (Auth)", 445, Some("test_user_1"))], &[]);
    assert!(usable.is_empty());
}

/// A repaired entry whose server is already saved folds into it: one row per server.
#[test]
fn a_repaired_entry_folds_into_the_one_already_saved() {
    let hosts = [bonjour("SMB Test (Auth)", Some("smb-auth"), None, 11482)];
    let usable = usable_entries(
        vec![
            entry("SMB Test (Auth)", 445, Some("test_user_1")),
            entry("smb-auth", 11482, None),
        ],
        &hosts,
    );
    assert_eq!(
        addresses(&usable),
        vec![("smb-auth".to_string(), 11482, Some("test_user_1".to_string()))]
    );
}

/// The read every consumer goes through drops a display-name entry, so nothing dials it.
#[test]
fn reading_the_store_sets_a_display_name_entry_aside() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(MANUAL_SERVERS_FILENAME);
    std::fs::write(
        &path,
        r#"{"servers": [
            {"id": "manual-SMB Test (Auth)-445", "address": "SMB Test (Auth)", "port": 445, "addedAt": "2026-10-01T10:00:00Z", "username": "test_user_1"},
            {"id": "manual-10-0-0-33-445", "address": "10.0.0.33", "port": 445, "addedAt": "2026-10-01T10:00:00Z"}
        ]}"#,
    )
    .expect("write");
    assert_eq!(
        addresses(&read_store_from_path(&path).servers),
        vec![("10.0.0.33".to_string(), 445, None)]
    );
}
