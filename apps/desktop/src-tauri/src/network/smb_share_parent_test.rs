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

fn share_on(server: &'static str, share: &'static str) -> impl FnOnce(&str) -> Option<SmbMountInfo> + Send + 'static {
    move |_| Some(mount(server, share))
}

/// The switch read back through the volume, the way the UI reaches it.
#[tokio::test]
async fn the_switch_reads_back_through_the_volume() {
    let dir = crate::test_support::TestDir::new("share_parent_roundtrip");
    let volume = Registered::at("roundtrip", &dir);
    let (server, share) = ("198.51.100.61", "roundtrip");
    let limit = Duration::from_secs(3);

    let before = share_parent_within(&volume.0, limit, share_on(server, share)).await;
    assert_eq!(before.as_ref().map(|parent| parent.show_parent_row), Some(None));
    assert_eq!(before.map(|parent| parent.share_name), Some(share.to_string()));

    assert!(set_parent_row_within(&volume.0, false, true, limit, share_on(server, share)).await);
    let off = share_parent_within(&volume.0, limit, share_on(server, share)).await;
    assert_eq!(off.map(|parent| parent.show_parent_row), Some(Some(false)));

    assert!(set_parent_row_within(&volume.0, true, true, limit, share_on(server, share)).await);
    let back = share_parent_within(&volume.0, limit, share_on(server, share)).await;
    assert_eq!(
        back.map(|parent| parent.show_parent_row),
        Some(None),
        "back to the default clears it"
    );
}

#[tokio::test]
async fn a_volume_with_no_share_behind_it_has_no_parent() {
    let limit = Duration::from_secs(3);
    assert!(
        share_parent_within("test-share-parent-missing", limit, |_| None)
            .await
            .is_none()
    );
    assert!(!set_parent_row_within("test-share-parent-missing", false, true, limit, |_| None).await);
}
