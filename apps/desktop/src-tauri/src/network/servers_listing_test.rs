//! What the Servers list lists, and what each row says.
//!
//! Ported from the hub's merge (`servers-hub-rows.test.ts`, `.s3.test.ts`), flat: a
//! many-place server's places are no rows of their own, and the pane sorts.

use std::collections::HashMap;

use cmdr_fs::server_meta::{ServerProtocol, ServerStatus, ShareListAccount};
use cmdr_fs::volume::ConnectionState;

use super::{SERVER_ICON_ID, SMB_SERVER_ICON_ID, ServerOpens, ServerRow, ServerRowInputs, server_rows};
use crate::network::share_list_path::ShareListAddress;
use crate::network::{HostSource, NetworkHost};
use crate::saved_servers::{SavedPlace, SavedServer, ServerNameSource};
use crate::volumes::SmbMountInfo;

/// Everything the rows read, owned, so a test states only what it's about.
#[derive(Default)]
struct Inputs {
    saved: Vec<SavedServer>,
    hosts: Vec<NetworkHost>,
    volume_states: HashMap<String, ConnectionState>,
    smb_mounts: Vec<SmbMountInfo>,
    listed_as: HashMap<String, ShareListAccount>,
}

impl Inputs {
    fn saved(saved: Vec<SavedServer>) -> Self {
        Self {
            saved,
            ..Self::default()
        }
    }

    fn hosts(mut self, hosts: Vec<NetworkHost>) -> Self {
        self.hosts = hosts;
        self
    }

    fn state(mut self, volume_id: &str, state: ConnectionState) -> Self {
        self.volume_states.insert(volume_id.to_string(), state);
        self
    }

    fn mount(mut self, server: &str, port: u16) -> Self {
        self.smb_mounts.push(SmbMountInfo {
            server: server.to_string(),
            share: "public".to_string(),
            subpath: None,
            username: None,
            port,
        });
        self
    }

    fn listed(mut self, host_id: &str, account: ShareListAccount) -> Self {
        self.listed_as.insert(host_id.to_string(), account);
        self
    }

    fn rows(&self) -> Vec<ServerRow> {
        server_rows(&ServerRowInputs {
            saved: &self.saved,
            hosts: &self.hosts,
            volume_states: &self.volume_states,
            smb_mounts: &self.smb_mounts,
            listed_as: &self.listed_as,
        })
    }
}

fn place(volume_id: &str, name: &str, app_root: &str, pinned: bool) -> SavedPlace {
    SavedPlace {
        volume_id: volume_id.to_string(),
        name: name.to_string(),
        pinned,
        connected: false,
        app_root: app_root.to_string(),
        username: Some("test_user_1".to_string()),
        auto_reconnect: Some(true),
    }
}

const SFTP_ID: &str = "sftp-testnas.local-22-test_user_1";

fn sftp_server() -> SavedServer {
    SavedServer {
        id: SFTP_ID.to_string(),
        protocol: ServerProtocol::Sftp,
        display_name: "Testnas".to_string(),
        name_source: ServerNameSource::User,
        address: "testnas.local:22".to_string(),
        username: Some("test_user_1".to_string()),
        pinned: true,
        last_connected_at: Some("2026-09-01T10:00:00Z".to_string()),
        auto_reconnect: Some(true),
        places: vec![place(SFTP_ID, "Testnas", "sftp://test_user_1@testnas.local:22", true)],
        awaiting_first_connect: false,
        saved_from_mount: false,
    }
}

fn sftp_named(id: &str, display_name: &str) -> SavedServer {
    SavedServer {
        id: id.to_string(),
        display_name: display_name.to_string(),
        places: vec![place(id, display_name, &format!("sftp://test_user_1@{id}:22"), true)],
        ..sftp_server()
    }
}

fn smb_server() -> SavedServer {
    SavedServer {
        id: "manual-10-0-0-4-445".to_string(),
        protocol: ServerProtocol::Smb,
        display_name: "Attic NAS".to_string(),
        name_source: ServerNameSource::Fallback,
        address: "10.0.0.4".to_string(),
        username: None,
        pinned: false,
        last_connected_at: None,
        auto_reconnect: None,
        places: Vec::new(),
        awaiting_first_connect: false,
        saved_from_mount: false,
    }
}

fn smb(id: &str, display_name: &str, address: &str) -> SavedServer {
    SavedServer {
        id: id.to_string(),
        display_name: display_name.to_string(),
        address: address.to_string(),
        ..smb_server()
    }
}

/// An SMB server Cmdr saved because a share from it was mounted: unnamed, so its label is
/// the address (`saved_servers::smb_hosts`).
fn from_mount(address: &str, port: u16) -> SavedServer {
    let label = crate::network::manual_servers::discovery_name(address, port);
    SavedServer {
        id: crate::network::manual_servers::generate_server_id(address, port),
        display_name: label.clone(),
        address: label,
        saved_from_mount: true,
        ..smb_server()
    }
}

fn host(id: &str, name: &str) -> NetworkHost {
    NetworkHost {
        id: id.to_string(),
        name: name.to_string(),
        hostname: None,
        ip_address: None,
        port: 445,
        source: HostSource::Discovered,
    }
}

fn manual(id: &str, name: &str) -> NetworkHost {
    NetworkHost {
        source: HostSource::Manual,
        ..host(id, name)
    }
}

fn names(rows: &[ServerRow]) -> Vec<&str> {
    rows.iter().map(|row| row.name.as_str()).collect()
}

fn named<'a>(rows: &'a [ServerRow], name: &str) -> &'a ServerRow {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("{name} must be a row"))
}

// --- Row identity -------------------------------------------------------------------

/// ❗ The listing keys entries by path and the frontend keys rows by id, and the sources
/// can genuinely repeat one: the saved listing unions several stores. So uniqueness is
/// this function's invariant, ❌ not its callers'.
#[test]
fn never_emits_two_rows_under_one_id_whatever_the_sources_repeat() {
    let rows = Inputs::saved(vec![
        smb("dup", "Attic NAS", "10.0.0.4"),
        smb("dup", "Attic NAS (again)", "10.0.0.9"),
        SavedServer {
            id: "dup".to_string(),
            ..sftp_server()
        },
    ])
    .hosts(vec![host("dup", "Somewhere else")])
    .rows();

    let mut ids: Vec<&str> = rows.iter().map(|row| row.meta.row_id.as_str()).collect();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count);
    assert_eq!(count, 1);
}

#[test]
fn keeps_the_first_row_under_a_repeated_id() {
    let rows = Inputs::saved(vec![
        smb("dup", "The real one", "10.0.0.4"),
        smb("dup", "The echo", "10.0.0.4"),
    ])
    .rows();
    assert_eq!(names(&rows), ["The real one"]);
}

/// ❗ Two rows that lead to one place would be two entries at one path, which the
/// listing engine keys on.
#[test]
fn never_emits_two_rows_that_lead_to_one_place() {
    let rows = Inputs::saved(vec![
        smb("manual-a", "First", "10.0.0.4"),
        smb("manual-b", "Second", "10.0.0.4"),
    ])
    .rows();
    assert_eq!(names(&rows), ["First"]);
}

// --- What appears -------------------------------------------------------------------

#[test]
fn lists_one_row_per_saved_server_and_one_per_discovered_host() {
    let rows = Inputs::saved(vec![sftp_server()])
        .hosts(vec![host("h9", "Someone else")])
        .rows();
    assert_eq!(names(&rows), ["Testnas", "Someone else"]);
    assert!(rows[0].meta.saved);
    assert!(!rows[1].meta.saved);
}

/// Adding a manual server injects it into the discovery state, so it comes back from
/// both sources with the same id.
#[test]
fn shows_a_manually_typed_smb_host_once() {
    let rows = Inputs::saved(vec![smb_server()])
        .hosts(vec![manual("manual-10-0-0-4-445", "Attic NAS")])
        .rows();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].meta.saved);
}

/// `known_shares` files a host under the server name statfs reported; mDNS files the
/// same machine under its Bonjour name and its own id.
#[test]
fn matches_a_saved_smb_host_to_a_discovered_one_by_name_when_the_ids_differ() {
    let rows = Inputs::saved(vec![smb("manual-testnas-445", "Testnas", "Testnas")])
        .hosts(vec![host("bonjour-1", "testnas")])
        .rows();
    assert_eq!(rows.len(), 1);
}

/// A share Finder mounted by Bonjour name is saved under its `.local` hostname, which is
/// the discovered host's: one row, under the Bonjour name, ❌ never a second row.
#[test]
fn matches_a_server_saved_off_a_bonjour_mount_to_the_discovered_host() {
    let rows = Inputs::saved(vec![from_mount("testnas.local", 445)])
        .hosts(vec![NetworkHost {
            hostname: Some("testnas.local".to_string()),
            ip_address: Some("10.0.0.4".to_string()),
            ..host("bonjour-testnas", "Testnas")
        }])
        .mount("Testnas._smb._tcp.local", 445)
        .rows();
    assert_eq!(names(&rows), ["Testnas"]);
    assert!(rows[0].meta.saved);
}

fn bonjour_testnas_at(ip: &str, port: u16) -> NetworkHost {
    NetworkHost {
        hostname: Some("testnas.local".to_string()),
        ip_address: Some(ip.to_string()),
        port,
        ..host("bonjour-testnas", "Testnas")
    }
}

/// ❗ A server saved by IP and the Bonjour host discovery resolved to exactly that address
/// and port are one row under the Bonjour name ("Naspolya" at `192.168.1.111`, David's
/// call after the mounted-servers work). The row stays the SAVED server's: its id, and the
/// share list at the address the person picked, so signing in on it never signs in to the
/// discovered host's own share list.
#[test]
fn merges_a_server_saved_by_ip_with_the_bonjour_host_at_that_address() {
    let saved = from_mount("10.0.0.4", 445);
    let rows = Inputs::saved(vec![saved.clone()])
        .hosts(vec![bonjour_testnas_at("10.0.0.4", 445)])
        .mount("10.0.0.4", 445)
        .rows();
    assert_eq!(names(&rows), ["Testnas"]);
    let row = &rows[0];
    assert!(row.meta.saved);
    assert_eq!(row.meta.row_id, saved.id);
    assert_eq!(row.meta.status, ServerStatus::Connected);
    assert_eq!(row.meta.address, "10.0.0.4");
    assert_eq!(
        row.opens,
        ServerOpens::ShareList(ShareListAddress {
            key: "10.0.0.4".to_string(),
            port: 445,
        })
    );
    assert_eq!(row.path, "smb://10.0.0.4");
}

/// Merged and unmounted, the row reads "Found nearby": mDNS is seeing that machine.
#[test]
fn a_server_saved_by_ip_reads_found_nearby_while_mdns_sees_that_address() {
    let rows = Inputs::saved(vec![from_mount("10.0.0.4", 445)])
        .hosts(vec![bonjour_testnas_at("10.0.0.4", 445)])
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::FoundNearby);
}

/// A name the person gave the server wins over the Bonjour name, merged or not.
#[test]
fn a_server_saved_by_ip_keeps_a_name_the_person_chose() {
    let named = SavedServer {
        name_source: ServerNameSource::User,
        ..smb("manual-10-0-0-4-445", "Attic NAS", "10.0.0.4")
    };
    let rows = Inputs::saved(vec![named])
        .hosts(vec![bonjour_testnas_at("10.0.0.4", 445)])
        .rows();
    assert_eq!(names(&rows), ["Attic NAS"]);
}

/// ❗ The port is part of the address: a host on another port at that IP is another
/// server (the Docker fixtures run ten on one address).
#[test]
fn keeps_a_server_saved_by_ip_apart_from_a_host_on_another_port_there() {
    let rows = Inputs::saved(vec![from_mount("10.0.0.4", 445)])
        .hosts(vec![bonjour_testnas_at("10.0.0.4", 10480)])
        .rows();
    assert_eq!(names(&rows), ["10.0.0.4", "Testnas"]);
}

/// ❗ Only the exact address merges: the same NAS by its Tailscale address stays a row of
/// its own (two addresses are two servers, § "Decisions", 5), ❌ never paired by GUID.
#[test]
fn keeps_a_server_saved_by_another_address_of_that_machine_apart() {
    let rows = Inputs::saved(vec![from_mount("10.0.0.4", 445), from_mount("100.64.0.10", 445)])
        .hosts(vec![bonjour_testnas_at("10.0.0.4", 445)])
        .rows();
    assert_eq!(names(&rows), ["Testnas", "100.64.0.10"]);
    assert_eq!(named(&rows, "100.64.0.10").path, "smb://100.64.0.10");
}

/// ❗ Servers behind mounted shares (§ "Decisions", 4 and 5 of the Servers plan): one NAS
/// mounted by its LAN and its Tailscale address is two saved servers, both connected,
/// whatever its GUID.
#[test]
fn lists_both_addresses_of_one_mounted_nas_as_two_connected_servers() {
    let rows = Inputs::saved(vec![from_mount("10.0.0.4", 445), from_mount("100.64.0.10", 445)])
        .mount("10.0.0.4", 445)
        .mount("100.64.0.10", 445)
        .rows();
    assert_eq!(names(&rows), ["10.0.0.4", "100.64.0.10"]);
    assert!(rows.iter().all(|row| row.meta.saved));
    assert!(rows.iter().all(|row| row.meta.status == ServerStatus::Connected));
}

/// Unmounted, a mount-saved server is still listed, as saved.
#[test]
fn keeps_a_server_saved_from_a_mount_after_its_share_unmounts() {
    let rows = Inputs::saved(vec![from_mount("100.64.0.10", 445)]).rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].meta.status, ServerStatus::Saved);
}

#[test]
fn keeps_two_servers_that_only_share_a_protocol_apart() {
    let rows = Inputs::saved(vec![sftp_server(), sftp_named("sftp-other-22-bo", "Jump box")]).rows();
    assert_eq!(rows.len(), 2);
}

/// ❗ One NAS reached by two addresses is two servers: nothing here pairs them, a GUID
/// least of all (#1518).
#[test]
fn keeps_one_nas_at_two_addresses_as_two_rows() {
    let rows = Inputs::saved(vec![
        smb("manual-192-168-1-111-445", "192.168.1.111", "192.168.1.111"),
        smb("manual-100-127-48-122-445", "100.127.48.122", "100.127.48.122"),
    ])
    .rows();
    assert_eq!(rows.len(), 2);
}

// --- Icons --------------------------------------------------------------------------

#[test]
fn draws_an_smb_server_as_a_filled_monitor_and_any_other_server_as_a_server() {
    let rows = Inputs::saved(vec![sftp_server()])
        .hosts(vec![host("h9", "Someone else")])
        .rows();
    let icons: Vec<&str> = rows.iter().map(|row| row.icon_id).collect();
    assert_eq!(icons, [SERVER_ICON_ID, SMB_SERVER_ICON_ID]);
}

// --- Whose name the Name column shows -----------------------------------------------

/// The label an SMB store carries: the mount's spelling, or a typed address.
fn stand_in() -> SavedServer {
    smb(
        "manual-smb-consumer-guest-445",
        "smb-consumer-guest",
        "smb-consumer-guest",
    )
}

/// Three ranks, and the top one is a fact the store publishes (`name_source`), ❌ never
/// a guess at the string's shape.
#[test]
fn keeps_the_name_the_person_chose_even_when_mdns_spells_the_host_differently() {
    let rows = Inputs::saved(vec![SavedServer {
        name_source: ServerNameSource::User,
        ..smb("manual-testnas-445", "Testnas", "testnas.local")
    }])
    .hosts(vec![NetworkHost {
        hostname: Some("testnas.local".to_string()),
        ..host("bonjour-1", "Testnas Media Server")
    }])
    .rows();
    assert_eq!(names(&rows), ["Testnas"]);
}

/// Opening a host writes a `known_shares` row named the way `statfs` spells the server,
/// and the friendly name a person recognizes must survive that.
#[test]
fn shows_the_bonjour_name_over_the_stand_in_which_nobody_chose() {
    let rows = Inputs::saved(vec![stand_in()])
        .hosts(vec![NetworkHost {
            hostname: Some("smb-consumer-guest".to_string()),
            ..host("bonjour-1", "SMB Test (Guest)")
        }])
        .rows();
    assert_eq!(names(&rows), ["SMB Test (Guest)"]);
}

#[test]
fn falls_back_to_the_stand_in_when_mdns_sees_nothing() {
    assert_eq!(names(&Inputs::saved(vec![stand_in()]).rows()), ["smb-consumer-guest"]);
}

/// ❗ Typing a host that mDNS already found puts the machine in the discovery list
/// TWICE, under both spellings. One row, named the way a person would recognize it.
#[test]
fn claims_both_spellings_of_one_machine_and_takes_the_discovered_name() {
    let rows = Inputs::saved(vec![stand_in()])
        .hosts(vec![
            manual("manual-smb-consumer-guest-445", "smb-consumer-guest"),
            NetworkHost {
                hostname: Some("smb-consumer-guest".to_string()),
                ..host("bonjour-1", "SMB Test (Guest)")
            },
        ])
        .rows();
    assert_eq!(names(&rows), ["SMB Test (Guest)"]);
}

// --- Status -------------------------------------------------------------------------

#[test]
fn calls_a_place_with_a_live_session_connected() {
    let rows = Inputs::saved(vec![sftp_server()])
        .state(SFTP_ID, ConnectionState::Direct)
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::Connected);
}

#[test]
fn calls_a_place_whose_credential_the_server_refused_signed_out() {
    let rows = Inputs::saved(vec![sftp_server()])
        .state(SFTP_ID, ConnectionState::NeedsSignIn)
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::SignedOut);
}

#[test]
fn calls_a_place_waiting_on_a_host_key_decision_waiting_for_the_key() {
    let rows = Inputs::saved(vec![sftp_server()])
        .state(SFTP_ID, ConnectionState::NeedsHostKeyApproval)
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::WaitingForKey);
}

#[test]
fn calls_a_saved_place_with_no_session_saved_dropped_session_included() {
    assert_eq!(
        Inputs::saved(vec![sftp_server()]).rows()[0].meta.status,
        ServerStatus::Saved
    );
    let dropped = Inputs::saved(vec![sftp_server()])
        .state(SFTP_ID, ConnectionState::Disconnected)
        .rows();
    assert_eq!(dropped[0].meta.status, ServerStatus::Saved);
}

#[test]
fn calls_a_host_only_mdns_knows_about_found_nearby() {
    let rows = Inputs::default().hosts(vec![host("h1", "Attic NAS")]).rows();
    assert_eq!(rows[0].meta.status, ServerStatus::FoundNearby);
}

#[test]
fn calls_a_saved_smb_host_found_nearby_while_mdns_sees_it() {
    let rows = Inputs::saved(vec![smb_server()])
        .hosts(vec![
            manual("manual-10-0-0-4-445", "Attic NAS"),
            host("attic-smb-tcp-local", "Attic NAS"),
        ])
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::FoundNearby);
}

/// ❗ A typed-in host is in the discovery list because Cmdr put it there (every saved one
/// is injected at startup, reachable or not), so its own entry says nothing about the
/// network. Reading it as "found nearby" called `localhost:11482` nearby the moment it
/// was added (QA 2026-09-25).
#[test]
fn calls_a_saved_smb_host_saved_when_the_only_host_behind_it_is_its_own_manual_entry() {
    let rows = Inputs::saved(vec![smb_server()])
        .hosts(vec![manual("manual-10-0-0-4-445", "Attic NAS")])
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::Saved);
}

/// ❗ A host saved without contact ("Add anyway", an approved Ask Cmdr card) reads so,
/// even when mDNS happens to see it: "Saved" with nothing listed read like an add that
/// didn't work (#777).
#[test]
fn calls_a_saved_smb_host_nobody_connected_yet_not_connected_yet_even_when_mdns_sees_it() {
    let waiting = SavedServer {
        awaiting_first_connect: true,
        saved_from_mount: false,
        ..smb_server()
    };
    assert_eq!(
        Inputs::saved(vec![waiting.clone()]).rows()[0].meta.status,
        ServerStatus::NotConnectedYet
    );
    let seen = Inputs::saved(vec![waiting])
        .hosts(vec![host("attic-smb-tcp-local", "Attic NAS")])
        .rows();
    assert_eq!(seen[0].meta.status, ServerStatus::NotConnectedYet);
}

/// `SavedPlace.connected` is a snapshot from the moment the listing was built; the
/// volume list is what the switcher and the pane both read.
#[test]
fn reads_a_places_standing_off_the_volume_list_not_the_saved_entrys_stale_flag() {
    let mut server = sftp_server();
    server.places[0].connected = true;
    assert_eq!(Inputs::saved(vec![server]).rows()[0].meta.status, ServerStatus::Saved);
}

/// An SMB server with a share mounted from it is connected, whoever mounted it (Cmdr,
/// Finder, `mount_smbfs`), recognized by address and port off the mount table.
#[test]
fn calls_an_smb_server_with_a_mounted_share_connected() {
    let rows = Inputs::saved(vec![smb_server()]).mount("10.0.0.4", 445).rows();
    assert_eq!(rows[0].meta.status, ServerStatus::Connected);
}

/// ❗ A mount wins over "Not connected yet": a share IS mounted from it.
#[test]
fn a_mount_wins_over_not_connected_yet() {
    let waiting = SavedServer {
        awaiting_first_connect: true,
        saved_from_mount: false,
        ..smb_server()
    };
    let rows = Inputs::saved(vec![waiting]).mount("10.0.0.4", 445).rows();
    assert_eq!(rows[0].meta.status, ServerStatus::Connected);
}

/// The mount table spells the server its own way (an IP); discovery pairs that with the
/// Bonjour name the row goes by.
#[test]
fn calls_a_found_host_connected_while_a_share_from_it_is_mounted_by_its_ip() {
    let rows = Inputs::default()
        .hosts(vec![NetworkHost {
            hostname: Some("testnas.local".to_string()),
            ip_address: Some("192.168.1.111".to_string()),
            ..host("bonjour-testnas", "Testnas")
        }])
        .mount("192.168.1.111", 445)
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::Connected);
}

/// ❗ One machine can run several SMB servers (the Docker fixtures run ten on
/// `localhost`): a mount from one port says nothing about another.
#[test]
fn a_mount_on_another_port_is_another_server() {
    let rows = Inputs::saved(vec![smb("manual-localhost-11482", "Both box", "localhost:11482")])
        .mount("localhost", 11481)
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::Saved);
    let same = Inputs::saved(vec![smb("manual-localhost-11482", "Both box", "localhost:11482")])
        .mount("localhost", 11482)
        .rows();
    assert_eq!(same[0].meta.status, ServerStatus::Connected);
}

#[test]
fn a_mount_from_another_server_leaves_the_row_alone() {
    let rows = Inputs::saved(vec![smb_server()]).mount("10.0.0.9", 445).rows();
    assert_eq!(rows[0].meta.status, ServerStatus::Saved);
}

// --- What a row carries -------------------------------------------------------------

#[test]
fn gives_a_one_place_server_its_places_volume_id_and_pin() {
    let rows = Inputs::saved(vec![sftp_server()]).rows();
    assert_eq!(rows[0].meta.place.as_deref(), Some(SFTP_ID));
    assert!(rows[0].meta.pinned);
    assert_eq!(
        rows[0].opens,
        ServerOpens::Place {
            volume_id: SFTP_ID.to_string(),
            app_root: "sftp://test_user_1@testnas.local:22".to_string(),
        }
    );
    assert_eq!(rows[0].last_connected_at.as_deref(), Some("2026-09-01T10:00:00Z"));
}

#[test]
fn gives_an_smb_host_no_place_its_shares_are_mounted_one_by_one() {
    let mut server = smb_server();
    server.places = vec![place("smb-container", "Container", "/Volumes/Container", true)];
    let rows = Inputs::saved(vec![server]).rows();
    assert_eq!(rows.len(), 1, "a saved share is no row of its own");
    assert_eq!(rows[0].meta.place, None);
    assert!(!rows[0].meta.pinned);
}

#[test]
fn shows_a_hosts_port_in_the_address_when_it_isnt_445() {
    let rows = Inputs::saved(vec![smb("manual-localhost-11482", "Both box", "localhost:11482")])
        .hosts(vec![NetworkHost {
            hostname: Some("localhost".to_string()),
            port: 11482,
            ..manual("manual-localhost-11482", "localhost:11482")
        }])
        .rows();
    assert_eq!(rows[0].meta.address, "localhost:11482");
}

#[test]
fn brackets_an_ipv6_address_before_its_port() {
    let rows = Inputs::default()
        .hosts(vec![NetworkHost {
            ip_address: Some("fe80::1".to_string()),
            port: 11482,
            ..host("h-v6", "Box")
        }])
        .rows();
    assert_eq!(rows[0].meta.address, "[fe80::1]:11482");
}

#[test]
fn prefers_the_discovered_hosts_resolved_address_over_the_saved_spelling() {
    let rows = Inputs::saved(vec![smb("manual-10-0-0-4-445", "Attic NAS", "Attic NAS")])
        .hosts(vec![NetworkHost {
            ip_address: Some("10.0.0.4".to_string()),
            ..host("manual-10-0-0-4-445", "Attic NAS")
        }])
        .rows();
    assert_eq!(rows[0].meta.address, "10.0.0.4");
}

// --- Where a row leads --------------------------------------------------------------

/// The share list a server row opens is the one `..` from that share list lands back
/// on: a host mDNS is seeing by its Bonjour name, folded.
#[test]
fn opens_a_server_mdns_is_seeing_by_its_bonjour_name() {
    let rows = Inputs::default().hosts(vec![host("h1", "Attic NAS")]).rows();
    assert_eq!(
        rows[0].opens,
        ServerOpens::ShareList(ShareListAddress {
            key: "attic nas".to_string(),
            port: 445,
        })
    );
}

#[test]
fn opens_a_saved_server_mdns_isnt_seeing_by_its_address_on_the_port_it_names() {
    let rows = Inputs::saved(vec![smb("manual-localhost-11482", "Both box", "localhost:11482")]).rows();
    assert_eq!(
        rows[0].opens,
        ServerOpens::ShareList(ShareListAddress {
            key: "localhost".to_string(),
            port: 11482,
        })
    );
    let plain = Inputs::saved(vec![smb_server()]).rows();
    assert_eq!(
        plain[0].opens,
        ServerOpens::ShareList(ShareListAddress {
            key: "10.0.0.4".to_string(),
            port: 445,
        })
    );
}

/// ❗ A row's path is where its entry sits in the `smb://` listing, which keys entries by
/// path: an SMB server's share-list path as the frontend writes it (so `..` from that
/// share list selects the row by it), a one-place server's place, an S3 account's root.
#[test]
fn a_rows_path_is_where_it_leads() {
    let rows = Inputs::saved(vec![
        sftp_server(),
        smb("manual-localhost-11482", "Both box", "localhost:11482"),
        s3_account(vec![bucket(Some("photos"))]),
    ])
    .hosts(vec![host("h1", "Attic NAS")])
    .rows();
    assert_eq!(named(&rows, "Testnas").path, "sftp://test_user_1@testnas.local:22");
    assert_eq!(named(&rows, "Both box").path, "smb://localhost:11482");
    assert_eq!(named(&rows, "Attic NAS").path, "smb://attic nas");
    // An account whose root isn't saved: the root its buckets sit under.
    assert_eq!(
        named(&rows, "AKIAEXAMPLE@127.0.0.1").path,
        "s3://AKIAEXAMPLE@127.0.0.1:14480/"
    );
}

/// ❗ No two rows share a path, whatever the sources repeat.
#[test]
fn never_emits_two_rows_at_one_path() {
    let rows = Inputs::saved(vec![smb("manual-a", "First", "Testnas"), sftp_server()])
        .hosts(vec![host("h1", "Testnas"), host("h2", "TESTNAS")])
        .rows();
    let mut paths: Vec<&str> = rows.iter().map(|row| row.path.as_str()).collect();
    let count = paths.len();
    paths.sort_unstable();
    paths.dedup();
    assert_eq!(paths.len(), count);
}

// --- The account a row is signed in as ----------------------------------------------

/// ❗ An SMB server row names the SERVER-level account, the same one its share list's
/// header names: the account its listing signed in as, else the one it's set to be used
/// with. ❌ Never a share's mount: "My NAS as guest" (from the one live guest mount)
/// disagreed with the header's "as testuser" (final QA).
#[test]
fn names_the_account_its_share_list_signed_in_as() {
    let rows = Inputs::saved(vec![smb_server()])
        .listed("manual-10-0-0-4-445", ShareListAccount::Guest)
        .rows();
    assert_eq!(rows[0].meta.account, Some(ShareListAccount::Guest));
}

#[test]
fn reads_the_listed_account_by_the_discovered_host_it_matched() {
    let rows = Inputs::saved(vec![smb_server()])
        .hosts(vec![host("attic-smb-tcp-local", "Attic NAS")])
        .listed(
            "attic-smb-tcp-local",
            ShareListAccount::User {
                username: "test_user_1".to_string(),
            },
        )
        .rows();
    assert_eq!(
        rows[0].meta.account,
        Some(ShareListAccount::User {
            username: "test_user_1".to_string()
        })
    );
}

#[test]
fn falls_back_to_the_account_it_is_set_to_be_used_with() {
    let server = SavedServer {
        username: Some("test_user_6".to_string()),
        ..smb_server()
    };
    let rows = Inputs::saved(vec![server]).mount("10.0.0.4", 445).rows();
    assert_eq!(
        rows[0].meta.account,
        Some(ShareListAccount::User {
            username: "test_user_6".to_string()
        })
    );
}

#[test]
fn says_nothing_when_nothing_is_known() {
    let rows = Inputs::saved(vec![smb_server()]).mount("10.0.0.4", 445).rows();
    assert_eq!(rows[0].meta.account, None);
}

#[test]
fn names_a_nearby_hosts_listing_account_too() {
    let rows = Inputs::default()
        .hosts(vec![host("h1", "Attic NAS")])
        .listed("h1", ShareListAccount::Guest)
        .rows();
    assert_eq!(rows[0].meta.account, Some(ShareListAccount::Guest));
}

/// A one-place server's name already says its account (`user@host`).
#[test]
fn a_one_place_server_names_no_account() {
    assert_eq!(Inputs::saved(vec![sftp_server()]).rows()[0].meta.account, None);
}

// --- S3 accounts --------------------------------------------------------------------

const ACCOUNT_ID: &str = "s3-127-0-0-1-14480-akiaexample-0000";

fn bucket(name: Option<&str>) -> SavedPlace {
    let (volume_id, label, root) = match name {
        None => (
            ACCOUNT_ID.to_string(),
            "AKIAEXAMPLE@127.0.0.1".to_string(),
            "s3://AKIAEXAMPLE@127.0.0.1:14480/".to_string(),
        ),
        Some(bucket) => (
            format!("s3-127-0-0-1-14480-akiaexample-{bucket}"),
            bucket.to_string(),
            format!("s3://AKIAEXAMPLE@127.0.0.1:14480/{bucket}"),
        ),
    };
    SavedPlace {
        username: Some("AKIAEXAMPLE".to_string()),
        ..place(&volume_id, &label, &root, true)
    }
}

fn s3_account(places: Vec<SavedPlace>) -> SavedServer {
    SavedServer {
        id: ACCOUNT_ID.to_string(),
        protocol: ServerProtocol::S3,
        display_name: "AKIAEXAMPLE@127.0.0.1".to_string(),
        name_source: ServerNameSource::Fallback,
        address: "http://127.0.0.1:14480".to_string(),
        username: Some("AKIAEXAMPLE".to_string()),
        pinned: false,
        last_connected_at: Some("2026-10-01T10:00:00Z".to_string()),
        auto_reconnect: Some(true),
        places,
        awaiting_first_connect: false,
        saved_from_mount: false,
    }
}

/// One row per account, its buckets no rows of their own (§ "Decisions", 6 of
/// `docs/specs/servers-list-as-listing-plan.md`).
#[test]
fn an_s3_account_is_one_row_with_no_place_or_pin_of_its_own() {
    let rows = Inputs::saved(vec![s3_account(vec![bucket(Some("scans")), bucket(Some("photos"))])]).rows();
    assert_eq!(names(&rows), ["AKIAEXAMPLE@127.0.0.1"]);
    let account = &rows[0];
    assert_eq!(account.meta.protocol, ServerProtocol::S3);
    assert_eq!(account.icon_id, SERVER_ICON_ID);
    // Its id is the ROOT place's id, saved or not, so a row that acted through it would
    // dial or pin the account root behind the person's back.
    assert_eq!(account.meta.place, None);
    assert!(!account.meta.pinned);
    assert_eq!(
        account.meta.account,
        Some(ShareListAccount::User {
            username: "AKIAEXAMPLE".to_string()
        })
    );
}

#[test]
fn an_s3_account_opens_its_root_whether_or_not_the_root_is_saved() {
    let unsaved = Inputs::saved(vec![s3_account(vec![bucket(Some("photos"))])]).rows();
    assert_eq!(
        unsaved[0].opens,
        ServerOpens::S3Account {
            root_volume_id: ACCOUNT_ID.to_string(),
            root_app_root: None,
        }
    );
    let saved = Inputs::saved(vec![s3_account(vec![bucket(Some("photos")), bucket(None)])]).rows();
    assert_eq!(
        saved[0].opens,
        ServerOpens::S3Account {
            root_volume_id: ACCOUNT_ID.to_string(),
            root_app_root: Some("s3://AKIAEXAMPLE@127.0.0.1:14480/".to_string()),
        }
    );
}

/// A live bucket makes the account live, the way a mounted share would.
#[test]
fn an_s3_account_reads_as_its_most_urgent_place() {
    let rows = Inputs::saved(vec![s3_account(vec![bucket(Some("photos")), bucket(Some("scans"))])])
        .state("s3-127-0-0-1-14480-akiaexample-scans", ConnectionState::NeedsSignIn)
        .rows();
    assert_eq!(rows[0].meta.status, ServerStatus::SignedOut);
    let live = Inputs::saved(vec![s3_account(vec![bucket(Some("photos")), bucket(Some("scans"))])])
        .state("s3-127-0-0-1-14480-akiaexample-photos", ConnectionState::Direct)
        .state("s3-127-0-0-1-14480-akiaexample-scans", ConnectionState::NeedsSignIn)
        .rows();
    assert_eq!(live[0].meta.status, ServerStatus::Connected);
}

#[test]
fn an_s3_account_keeps_a_name_a_person_chose() {
    let account = SavedServer {
        display_name: "Work".to_string(),
        name_source: ServerNameSource::User,
        ..s3_account(vec![bucket(None)])
    };
    assert_eq!(names(&Inputs::saved(vec![account]).rows()), ["Work"]);
}

#[test]
fn a_saved_and_a_found_row_carry_their_own_ids() {
    let rows = Inputs::saved(vec![sftp_server()])
        .hosts(vec![host("h9", "Someone else")])
        .rows();
    assert_eq!(named(&rows, "Testnas").meta.row_id, SFTP_ID);
    assert_eq!(named(&rows, "Someone else").meta.row_id, "h9");
}
