//! Tests for `smb_share_parent.rs`: the override-pruning rule, the identity match, and
//! which host `..` opens.

use super::*;

fn host(id: &str, name: &str, hostname: Option<&str>, ip: Option<&str>, source: HostSource) -> NetworkHost {
    NetworkHost {
        id: id.to_string(),
        name: name.to_string(),
        hostname: hostname.map(str::to_string),
        ip_address: ip.map(str::to_string),
        port: 445,
        source,
    }
}

fn mount(server: &str, share: &str) -> SmbMountInfo {
    SmbMountInfo {
        server: server.to_string(),
        share: share.to_string(),
        subpath: None,
        username: None,
        port: 445,
    }
}

#[test]
fn a_choice_matching_the_default_is_not_stored() {
    let mut overrides = Vec::new();
    choose_parent_row(&mut overrides, "198.51.100.7", "media", true, true, &[]);
    assert!(overrides.is_empty(), "on, with the default on, is no override");
    assert_eq!(parent_row_override(&overrides, "198.51.100.7", "media", &[]), None);
}

#[test]
fn a_choice_differing_from_the_default_is_stored_and_read_back() {
    let mut overrides = Vec::new();
    choose_parent_row(&mut overrides, "198.51.100.7", "media", false, true, &[]);
    assert_eq!(
        parent_row_override(&overrides, "198.51.100.7", "media", &[]),
        Some(false)
    );
    assert_eq!(
        parent_row_override(&overrides, "198.51.100.7", "other", &[]),
        None,
        "per share"
    );
}

#[test]
fn flipping_back_to_the_default_clears_the_override() {
    let mut overrides = Vec::new();
    choose_parent_row(&mut overrides, "198.51.100.7", "media", false, true, &[]);
    choose_parent_row(&mut overrides, "198.51.100.7", "media", true, true, &[]);
    assert!(overrides.is_empty());
}

#[test]
fn a_share_keeps_one_entry_whatever_its_name_is_spelled() {
    let mut overrides = Vec::new();
    choose_parent_row(&mut overrides, "198.51.100.7", "Media", true, false, &[]);
    choose_parent_row(&mut overrides, "198.51.100.7", "media", true, false, &[]);
    assert_eq!(overrides.len(), 1, "share names fold case");
}

/// `statfs` spells one server by its IP on one mount and its Bonjour name on the next.
#[test]
fn an_override_follows_the_server_under_another_name() {
    let hosts = [host(
        "testnas",
        "Testnas",
        Some("testnas.local"),
        Some("198.51.100.7"),
        HostSource::Discovered,
    )];
    let mut overrides = Vec::new();
    choose_parent_row(&mut overrides, "198.51.100.7", "media", false, true, &hosts);
    assert_eq!(
        parent_row_override(&overrides, "testnas.local", "media", &hosts),
        Some(false)
    );
}

#[test]
fn the_host_is_the_discovered_one_when_discovery_knows_the_server() {
    let hosts = [
        host(
            "manual",
            "198.51.100.7",
            Some("198.51.100.7"),
            Some("198.51.100.7"),
            HostSource::Manual,
        ),
        host(
            "testnas",
            "Testnas",
            Some("testnas.local"),
            Some("198.51.100.7"),
            HostSource::Discovered,
        ),
    ];
    assert_eq!(host_for_mount(&mount("198.51.100.7", "media"), &hosts).id, "testnas");
}

#[test]
fn the_host_is_made_from_the_mount_when_discovery_knows_nothing() {
    let made = host_for_mount(&mount("198.51.100.9", "media"), &[]);
    assert_eq!(made.hostname.as_deref(), Some("198.51.100.9"));
    assert_eq!(made.port, 445);
    assert_eq!(made.source, HostSource::Manual);
}

#[test]
fn a_host_on_another_port_is_not_the_server() {
    let mut other_port = host(
        "other",
        "198.51.100.7:11480",
        Some("198.51.100.7"),
        None,
        HostSource::Manual,
    );
    other_port.port = 11480;
    let found = host_for_mount(&mount("198.51.100.7", "media"), &[other_port]);
    assert_ne!(found.id, "other", "one machine can run several servers");
}

/// A volume registered for the length of one test, under an id no other test uses.
struct Registered(String);

impl Registered {
    fn at(label: &str, root: &std::path::Path) -> Self {
        use crate::file_system::volume::manager::get_volume_manager;
        let id = format!("test-share-parent-{label}");
        get_volume_manager().register(
            &id,
            std::sync::Arc::new(cmdr_local::LocalVolume::local_folder(
                label,
                root,
                crate::volume_host::host(),
            )),
        );
        Self(id)
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        crate::file_system::volume::manager::get_volume_manager().unregister(&self.0);
    }
}

fn share_on(server: &'static str, share: &'static str) -> impl FnOnce(&str) -> Option<SmbMountInfo> {
    move |_| Some(mount(server, share))
}

/// The switch read back through the volume, the way the UI reaches it.
#[test]
fn the_switch_reads_back_through_the_volume() {
    let dir = crate::test_support::TestDir::new("share_parent_roundtrip");
    let volume = Registered::at("roundtrip", &dir);
    let (server, share) = ("198.51.100.61", "roundtrip");

    let before = share_parent_with(&volume.0, share_on(server, share));
    assert_eq!(before.as_ref().map(|parent| parent.show_parent_row), Some(None));
    assert_eq!(before.map(|parent| parent.share_name), Some(share.to_string()));

    assert!(set_parent_row_with(&volume.0, false, true, share_on(server, share)));
    let off = share_parent_with(&volume.0, share_on(server, share));
    assert_eq!(off.map(|parent| parent.show_parent_row), Some(Some(false)));

    assert!(set_parent_row_with(&volume.0, true, true, share_on(server, share)));
    let back = share_parent_with(&volume.0, share_on(server, share));
    assert_eq!(
        back.map(|parent| parent.show_parent_row),
        Some(None),
        "back to the default clears it"
    );
}

#[test]
fn a_volume_with_no_share_behind_it_has_no_parent() {
    assert!(share_parent_with("test-share-parent-missing", share_on("198.51.100.62", "gone")).is_none());
    assert!(!set_parent_row_with("test-share-parent-missing", false, true, |_| None));
}

/// The share is read at the volume's OWN root: a saved share the pane landed on before
/// its mount finished (#1238's missing `..`) has nothing there yet, so it answers `None`
/// for now, and the top-folder listing that follows the mount answers again.
#[test]
fn a_volume_whose_mount_isnt_up_yet_has_no_parent_yet() {
    let dir = crate::test_support::TestDir::new("share_parent_unmounted");
    let volume = Registered::at("unmounted", &dir);
    let asked = std::cell::RefCell::new(None);
    let parent = share_parent_with(&volume.0, |at| {
        asked.replace(Some(at.to_string()));
        None
    });
    assert!(parent.is_none());
    let asked = asked.into_inner().expect("the volume's root is read");
    assert!(asked.contains("share_parent_unmounted"), "read at {asked}");
}

/// What the share list resolves `..`'s host to, the way the app knows its servers:
/// discovery's `fresh` hosts, the manual store, and the SMB mounts the kernel lists.
fn resolved_share_list(
    info: &SmbMountInfo,
    fresh: Vec<NetworkHost>,
    saved: Vec<NetworkHost>,
) -> (NetworkHost, Option<NetworkHost>) {
    use crate::network::share_list_path::{KnownHosts, ShareListAddress, resolve};
    let host = host_for_mount(info, &fresh);
    let known = KnownHosts {
        fresh,
        saved,
        mounted: mounted_hosts(&[("/Volumes/share".to_string(), info.clone())]),
        cached: Vec::new(),
    };
    let found = resolve(&ShareListAddress::of_host(&host), &known);
    (host, found)
}

fn on_port(mut info: SmbMountInfo, port: u16) -> SmbMountInfo {
    info.port = port;
    info
}

/// ❗ `..` out of a share opens the share list of the host it names, so every kind of
/// server a share can be mounted from must be one that share list resolves, under the
/// same host. A share macOS mounted by a Tailscale IP nothing else knows read "Path not
/// found: smb://100.127.48.122/" (QA, 2026-11-26).
#[test]
fn every_kind_of_server_dot_dot_leads_to_is_one_its_share_list_resolves() {
    use crate::network::manual_servers::create_network_host;
    let bonjour = host(
        "testnas",
        "Testnas",
        Some("testnas.local"),
        Some("198.51.100.7"),
        HostSource::Discovered,
    );
    let typed_ip = create_network_host("198.51.100.8", 445);
    let typed_name = create_network_host("nas.example.test", 445);
    let typed_port = create_network_host("198.51.100.10", 11480);

    let cases: Vec<(&str, SmbMountInfo, Vec<NetworkHost>, Vec<NetworkHost>)> = vec![
        (
            "a Bonjour server, mounted by its IP",
            mount("198.51.100.7", "media"),
            vec![bonjour.clone()],
            vec![],
        ),
        (
            "a manual server by IP, before discovery loads it",
            mount("198.51.100.8", "media"),
            vec![],
            vec![typed_ip.clone()],
        ),
        (
            "a manual server by IP, pinned in discovery",
            mount("198.51.100.8", "media"),
            vec![typed_ip.clone()],
            vec![typed_ip],
        ),
        (
            "a manual server by hostname",
            mount("nas.example.test", "media"),
            vec![],
            vec![typed_name],
        ),
        (
            "a manual server on a custom port",
            on_port(mount("198.51.100.10", "media"), 11480),
            vec![],
            vec![typed_port],
        ),
        // The Bonjour name discovery sees is the same box on its LAN address, which only
        // its GUID could tell: the Tailscale IP is a server of its own.
        (
            "a Tailscale IP nothing else knows",
            mount("100.64.0.9", "media"),
            vec![bonjour],
            vec![],
        ),
        (
            "an unknown server on a custom port",
            on_port(mount("198.51.100.9", "media"), 11481),
            vec![],
            vec![],
        ),
    ];
    for (why, info, fresh, saved) in cases {
        let (host, found) = resolved_share_list(&info, fresh, saved);
        let found = found.unwrap_or_else(|| panic!("{why}: its share list resolves nothing"));
        assert_eq!(found.id, host.id, "{why}: resolves to the host `..` opened");
        assert_eq!(found.port, info.port, "{why}: on the mount's port");
    }
}
