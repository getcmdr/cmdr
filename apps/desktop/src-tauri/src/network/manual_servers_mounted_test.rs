//! Servers saved from the mount table: which mounts save one, under what spelling, and
//! that a second pass saves nothing new.

use super::super::{
    HostEdit, ManualServerEntry, add_server_entry_to_path, contact::note_first_connect_at_path, discoverable,
    generate_server_id, read_store_from_path,
};
use super::*;
use crate::network::known_shares::{AuthOptions, ConnectionMode, KnownNetworkShare};
use crate::network::server_guid::GuidProof;
use crate::network::{HostSource, NetworkHost};

fn temp_store() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(MANUAL_SERVERS_FILENAME);
    (dir, path)
}

fn mount(server: &str, port: u16, share: &str) -> SmbMountInfo {
    SmbMountInfo {
        server: server.to_string(),
        share: share.to_string(),
        subpath: None,
        username: Some("test_user_1".to_string()),
        port,
    }
}

fn typed(address: &str, port: u16) -> ManualServerEntry {
    ManualServerEntry {
        id: generate_server_id(address, port),
        display_name: "Attic NAS".to_string(),
        address: address.to_string(),
        port,
        added_at: "2026-11-01T00:00:00Z".to_string(),
        username: None,
        awaiting_first_connect: false,
        saved_from_mount: false,
    }
}

fn history(server_name: &str, address: Option<&str>, port: Option<u16>) -> KnownNetworkShare {
    KnownNetworkShare {
        server_name: server_name.to_string(),
        share_name: "public".to_string(),
        protocol: "smb".to_string(),
        last_connected_at: "2026-11-01T10:00:00Z".to_string(),
        last_connection_mode: ConnectionMode::Guest,
        last_known_auth_options: AuthOptions::GuestOrCredentials,
        username: None,
        address: address.map(str::to_string),
        port,
        volume_id: None,
        mount_path: None,
        pinned: false,
        server_guid: None,
        server_guid_proof: GuidProof::Unauthenticated,
    }
}

fn discovered(name: &str, hostname: &str, ip: &str) -> NetworkHost {
    NetworkHost {
        id: format!("bonjour-{name}"),
        name: name.to_string(),
        hostname: Some(hostname.to_string()),
        ip_address: Some(ip.to_string()),
        port: 445,
        source: HostSource::Discovered,
    }
}

fn addresses(servers: &[SmbServer]) -> Vec<(String, u16)> {
    servers
        .iter()
        .map(|server| (server.host().to_string(), server.port()))
        .collect()
}

/// ❗ One NAS mounted by its LAN and its Tailscale address is two servers (#1518: a
/// GUID proves a name, not a machine), and each is saved as the mount spells it.
#[test]
fn saves_one_nas_mounted_by_two_addresses_as_two_servers() {
    let mounts = [mount("10.0.0.4", 445, "public"), mount("100.64.0.10", 445, "public")];
    let to_save = servers_to_save(&mounts, &[], &[], &[]);
    assert_eq!(
        addresses(&to_save),
        vec![("10.0.0.4".to_string(), 445), ("100.64.0.10".to_string(), 445)]
    );
}

/// Several shares of one server save it once.
#[test]
fn saves_a_server_once_however_many_of_its_shares_are_mounted() {
    let mounts = [mount("10.0.0.4", 445, "public"), mount("10.0.0.4", 445, "media")];
    assert_eq!(servers_to_save(&mounts, &[], &[], &[]).len(), 1);
}

/// The port is part of the server: two fixtures on one machine are two servers.
#[test]
fn keeps_the_port_a_mount_names() {
    let mounts = [mount("localhost", 10480, "public"), mount("localhost", 10481, "public")];
    assert_eq!(
        addresses(&servers_to_save(&mounts, &[], &[], &[])),
        vec![("localhost".to_string(), 10480), ("localhost".to_string(), 10481)]
    );
}

/// A server the person typed is theirs already, under whatever name discovery pairs
/// with the mount's spelling.
#[test]
fn saves_nothing_for_a_server_already_typed_in() {
    let hosts = [discovered("Testnas", "testnas.local", "10.0.0.4")];
    let to_save = servers_to_save(
        &[mount("10.0.0.4", 445, "public")],
        &[typed("testnas.local", 445)],
        &[],
        &hosts,
    );
    assert!(to_save.is_empty(), "the mount's IP is the typed host's");
}

/// A server the share history already lists (a mount through Cmdr, a share list someone
/// signed in to) has its row.
#[test]
fn saves_nothing_for_a_server_the_share_history_lists() {
    let by_address = [history("Testnas", Some("10.0.0.4"), None)];
    assert!(servers_to_save(&[mount("10.0.0.4", 445, "public")], &[], &by_address, &[]).is_empty());
    let by_name = [history("testnas.local", None, None)];
    assert!(servers_to_save(&[mount("testnas.local", 445, "public")], &[], &by_name, &[]).is_empty());
}

/// ❗ An mDNS service name in `statfs` is keyed by its `.local` hostname, ❌ never by the
/// address discovery resolves it to: that flips with discovery timing, and the next
/// launch would save the same NAS again.
#[test]
fn keys_a_service_name_mount_by_its_local_hostname_whatever_discovery_knows() {
    let mounts = [mount("Testnas._smb._tcp.local", 445, "public")];
    assert_eq!(
        addresses(&servers_to_save(&mounts, &[], &[], &[])),
        vec![("testnas.local".to_string(), 445)]
    );
    let hosts = [discovered("Testnas", "testnas.local", "10.0.0.4")];
    assert_eq!(
        addresses(&servers_to_save(&mounts, &[], &[], &hosts)),
        vec![("testnas.local".to_string(), 445)]
    );
}

/// ❗ Idempotent across passes (launches, mount events): the second pass, with discovery
/// now pairing the name and the IP, saves nothing new, and the entry is unnamed, marked,
/// and out of the discovery list.
#[test]
fn a_second_pass_saves_nothing_new() {
    let (_dir, path) = temp_store();
    let mounts = [
        mount("Testnas._smb._tcp.local", 445, "public"),
        mount("10.0.0.9", 445, "media"),
    ];

    let first = servers_to_save(&mounts, &read_store_from_path(&path).servers, &[], &[]);
    assert_eq!(save_at_path(&path, &first), 2);

    let hosts = [discovered("Testnas", "testnas.local", "10.0.0.4")];
    let second = servers_to_save(&mounts, &read_store_from_path(&path).servers, &[], &hosts);
    assert!(second.is_empty(), "{second:?}");
    assert_eq!(save_at_path(&path, &first), 0, "a stale batch writes nothing twice");

    let store = read_store_from_path(&path);
    assert_eq!(store.servers.len(), 2);
    for entry in &store.servers {
        assert!(entry.saved_from_mount);
        assert!(!entry.is_named(), "a name a person types outranks it");
        assert!(entry.username.is_none(), "only a person sets the account");
    }
    assert_eq!(
        discoverable(&store.servers).count(),
        0,
        "nothing lists their shares unasked"
    );
}

/// A mount through Cmdr makes a mount-saved server ordinary, like a host saved without
/// contact.
#[test]
fn a_mount_through_cmdr_makes_it_ordinary() {
    let (_dir, path) = temp_store();
    save_at_path(&path, &[SmbServer::new("10.0.0.4", 445)]);
    assert!(note_first_connect_at_path(&path, &SmbServer::new("10.0.0.4", 445), &[]));
    let store = read_store_from_path(&path);
    assert!(!store.servers[0].saved_from_mount);
    assert_eq!(discoverable(&store.servers).count(), 1);
}

/// A person adding the same address makes it theirs.
#[test]
fn a_persons_add_of_the_address_makes_it_ordinary() {
    let (_dir, path) = temp_store();
    save_at_path(&path, &[SmbServer::new("10.0.0.4", 445)]);
    add_server_entry_to_path(&path, typed("10.0.0.4", 445));
    let store = read_store_from_path(&path);
    assert_eq!(store.servers.len(), 1);
    assert!(!store.servers[0].saved_from_mount);
    assert_eq!(store.servers[0].label(), "Attic NAS");
}

/// ❗ § "Decisions", 5 (b): one NAS at two addresses stays two servers, and Edit server
/// changes one's address but never merges it into the other (the relocate refusal).
/// The edit is the person's, so the moved server is theirs.
#[test]
fn editing_one_address_of_a_mounted_nas_never_merges_it_into_the_other() {
    use super::super::relocate::relocate_at_path;

    let (_dir, path) = temp_store();
    save_at_path(
        &path,
        &[SmbServer::new("10.0.0.4", 445), SmbServer::new("100.64.0.10", 445)],
    );
    let tailscale = generate_server_id("100.64.0.10", 445);

    let refused = relocate_at_path(
        &path,
        &tailscale,
        &SmbServer::new("10.0.0.4", 445),
        &HostEdit::default(),
    );
    assert_eq!(refused.map(|entry| entry.id), Err("10.0.0.4".to_string()));
    assert_eq!(read_store_from_path(&path).servers.len(), 2, "nothing merged");

    let moved = relocate_at_path(
        &path,
        &tailscale,
        &SmbServer::new("10.0.0.5", 445),
        &HostEdit::default(),
    )
    .expect("nothing holds the new address");
    assert!(!moved.saved_from_mount);
    assert_eq!(read_store_from_path(&path).servers.len(), 2);
}
