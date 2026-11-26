//! The Servers list's rows: every server the person saved, plus every host mDNS found,
//! one flat row each.
//!
//! A pure fold over inputs the caller hands in (the saved listing, discovery's fresh
//! hosts, the volume list's connection states, the kernel's SMB mount table, and the
//! accounts share lists signed in as), so ❌ nothing here dials, reads the Keychain, or
//! touches a mount (#324). The volume that lists `smb://` builds the inputs; a test
//! injects every one.
//!
//! ❗ **The merge is the part that goes quietly wrong.** A manually typed SMB host is
//! BOTH a saved server and a discovered host (adding one injects it into the discovery
//! state), so concatenating the two sources shows a person's NAS twice. The dedup
//! matches on the id first (a manual host keeps its store id in the discovery list),
//! then on the name or resolved hostname, because `known_shares` files a host under the
//! server name `statfs` reported while mDNS files the same machine under its Bonjour
//! name. It claims EVERY host that matches, since one machine can be in the discovery
//! list under both spellings. That match compares lowercase strings on purpose: it pairs
//! a saved LABEL with a Bonjour name, which no address-and-port identity can answer.
//!
//! ❗ A server saved by IP is NOT claimed by the Bonjour host discovery resolved to that
//! IP: an address is the person's pick (§ "Decisions", 5 of the Servers plan), and the
//! claimed host is what the row opens, signs in to, and prefetches (the E2E suite's
//! auth-by-IP server keeps the discovered `SMB Test (Auth)` untouched that way).
//!
//! ❌ Never merge two rows by server GUID: Samba derives its GUID from the NetBIOS name,
//! so it proves a name, not a machine (#1518). Two addresses of one NAS are two rows.

use std::collections::{HashMap, HashSet};

use cmdr_fs::server_meta::{ServerMeta, ServerProtocol, ServerStatus, ShareListAccount};
use cmdr_fs::volume::ConnectionState;

use crate::network::server_identity::{SmbServer, mount_is_from, smb_servers_of};
use crate::network::share_list_path::ShareListAddress;
use crate::network::{HostSource, NetworkHost};
use crate::saved_servers::{SavedPlace, SavedServer, ServerNameSource};
use crate::volumes::SmbMountInfo;

/// SMB's own port, which an address leaves unsaid.
const SMB_PORT: u16 = 445;

/// The icon id an SMB server's row carries: a machine (a monitor with a filled screen,
/// as macOS draws one). The frontend draws it, ❌ never an OS icon fetch.
pub(crate) const SMB_SERVER_ICON_ID: &str = "network:server-smb";

/// The icon id every other server's row carries: a service on a machine.
pub(crate) const SERVER_ICON_ID: &str = "network:server";

/// What the Servers list reads to build its rows. Memory only, ❌ never the wire.
pub(crate) struct ServerRowInputs<'a> {
    /// `saved_servers::listing`, the union of every store.
    pub saved: &'a [SavedServer],
    /// Discovery's current hosts (`fresh_discovered_hosts`), manual entries included.
    pub hosts: &'a [NetworkHost],
    /// The volume list's connection state per volume id; a volume with none is absent.
    pub volume_states: &'a HashMap<String, ConnectionState>,
    /// The kernel's SMB mount table (`volumes::smb_mounts`), ❌ never a `statfs` per mount.
    pub smb_mounts: &'a [SmbMountInfo],
    /// The account each host's share list last signed in as, by host id: a snapshot of
    /// `SmbServerVolume`'s, taken without holding its lock across an await.
    pub listed_as: &'a HashMap<String, ShareListAccount>,
}

/// One row of the Servers list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerRow {
    /// What the Name column shows.
    pub name: String,
    /// The facts the row's cells, dot, and actions read.
    pub meta: ServerMeta,
    /// ISO 8601, or `None` when nothing ever recorded one: the "Last used" date.
    pub last_connected_at: Option<String>,
    /// [`SMB_SERVER_ICON_ID`] or [`SERVER_ICON_ID`].
    pub icon_id: &'static str,
    /// Where Enter on the row leads, which is also what its path is built from.
    pub opens: ServerOpens,
}

/// Where a row leads. ❗ Two rows never lead to one place ([`server_rows`] keeps the
/// first): the listing keys entries by path, and the path comes from this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServerOpens {
    /// An SMB server: its share list, at this address.
    ShareList(ShareListAddress),
    /// A one-place server (SFTP, WebDAV, NFS): its place.
    Place {
        /// The place's volume id.
        volume_id: String,
        /// The app root a pane lands on.
        app_root: String,
    },
    /// An S3 account: its root, which lists the buckets (§ "Decisions", 6 of
    /// `docs/specs/servers-list-as-listing-plan.md`).
    S3Account {
        /// The root place's volume id, which is the account's id, saved or not.
        root_volume_id: String,
        /// The root's app root when the root is saved, `None` when only buckets are.
        root_app_root: Option<String>,
    },
}

impl ServerOpens {
    /// The key two rows must never share.
    fn key(&self) -> String {
        match self {
            Self::ShareList(address) => format!("smb\u{0}{}\u{0}{}", address.key, address.port),
            Self::Place { volume_id, .. } => format!("place\u{0}{volume_id}"),
            Self::S3Account { root_volume_id, .. } => format!("place\u{0}{root_volume_id}"),
        }
    }
}

/// The Servers list's rows, flat: one per saved server, then one per host only mDNS
/// knows. The pane sorts them.
///
/// ❗ **Every row's id AND every row's target are unique, and that is this function's
/// job, ❌ not its caller's.** `saved_servers::listing` unions several stores, and a
/// server recorded in two of them arrives twice. First writer wins, so saved rows win
/// over found ones.
pub(crate) fn server_rows(inputs: &ServerRowInputs<'_>) -> Vec<ServerRow> {
    let mut claimed: HashSet<&str> = HashSet::new();
    let mut taken_ids: HashSet<String> = HashSet::new();
    let mut taken_targets: HashSet<String> = HashSet::new();
    let mut rows = Vec::new();
    let mut add = |row: ServerRow| {
        if taken_ids.contains(&row.meta.row_id) || taken_targets.contains(&row.opens.key()) {
            return;
        }
        taken_ids.insert(row.meta.row_id.clone());
        taken_targets.insert(row.opens.key());
        rows.push(row);
    };

    for server in inputs.saved {
        let hosts = matching_hosts(server, inputs.hosts);
        claimed.extend(hosts.iter().map(|host| host.id.as_str()));
        if let Some(row) = saved_row(server, &hosts, inputs) {
            add(row);
        }
    }
    for host in inputs.hosts {
        if !claimed.contains(host.id.as_str()) {
            add(nearby_row(host, inputs));
        }
    }
    rows
}

/// Whether a protocol's server holds many places: an SMB host's shares, an S3 account's
/// buckets. Its row opens no single place.
fn has_many_places(protocol: ServerProtocol) -> bool {
    matches!(protocol, ServerProtocol::Smb | ServerProtocol::S3)
}

/// Every host in the discovery list that IS this saved SMB server.
///
/// ❗ Plural on purpose: ONE machine can be in that list twice, because adding a host by
/// hand injects a `manual` host beside the `discovered` one mDNS already found. Claiming
/// only the first would leave the other as a second row for the same NAS.
fn matching_hosts<'a>(server: &SavedServer, hosts: &'a [NetworkHost]) -> Vec<&'a NetworkHost> {
    if server.protocol != ServerProtocol::Smb {
        return Vec::new();
    }
    let address = server.address.to_lowercase();
    let name = server.display_name.to_lowercase();
    hosts
        .iter()
        .filter(|host| {
            let host_name = host.name.to_lowercase();
            host.id == server.id
                || host_name == address
                || host_name == name
                || host.hostname.as_deref().map(str::to_lowercase).as_deref() == Some(address.as_str())
        })
        .collect()
}

/// Which matching host the row speaks for. A DISCOVERED host wins: its name is the
/// Bonjour name a person recognizes, where a manual host is named after the address
/// they typed.
fn primary_host<'a>(hosts: &[&'a NetworkHost]) -> Option<&'a NetworkHost> {
    hosts
        .iter()
        .find(|host| host.source == HostSource::Discovered)
        .or_else(|| hosts.first())
        .copied()
}

fn saved_row(server: &SavedServer, hosts: &[&NetworkHost], inputs: &ServerRowInputs<'_>) -> Option<ServerRow> {
    let host = primary_host(hosts);
    let many = has_many_places(server.protocol);
    let place: Option<&SavedPlace> = server.places.first();
    let account = match server.protocol {
        ServerProtocol::Smb => host
            .and_then(|host| inputs.listed_as.get(&host.id))
            .or_else(|| inputs.listed_as.get(&server.id))
            .cloned()
            .or_else(|| user(server.username.as_deref())),
        ServerProtocol::S3 => user(server.username.as_deref()),
        ServerProtocol::Sftp | ServerProtocol::Webdav | ServerProtocol::Nfs => None,
    };
    let opens = match server.protocol {
        ServerProtocol::Smb => ServerOpens::ShareList(ShareListAddress::of_host(
            &host.cloned().unwrap_or_else(|| saved_host(server)),
        )),
        ServerProtocol::S3 => ServerOpens::S3Account {
            root_volume_id: server.id.clone(),
            root_app_root: server
                .places
                .iter()
                .find(|place| place.volume_id == server.id)
                .map(|root| root.app_root.clone()),
        },
        ServerProtocol::Sftp | ServerProtocol::Webdav | ServerProtocol::Nfs => {
            let Some(place) = place else {
                log::warn!(target: "network", "a saved one-place server has no place to open; leaving it off the Servers list");
                return None;
            };
            ServerOpens::Place {
                volume_id: place.volume_id.clone(),
                app_root: place.app_root.clone(),
            }
        }
    };
    Some(ServerRow {
        name: display_name(server, host),
        meta: ServerMeta {
            row_id: server.id.clone(),
            protocol: server.protocol,
            address: host.and_then(host_address).unwrap_or_else(|| server.address.clone()),
            status: saved_status(server, hosts, host, inputs),
            account,
            saved: true,
            // A many-place server's places are its own; the row opens none of them.
            place: (!many).then(|| place.map(|place| place.volume_id.clone())).flatten(),
            pinned: !many && place.is_some_and(|place| place.pinned),
        },
        last_connected_at: server.last_connected_at.clone(),
        icon_id: icon_for(server.protocol),
        opens,
    })
}

fn nearby_row(host: &NetworkHost, inputs: &ServerRowInputs<'_>) -> ServerRow {
    let mounted = is_mounted(&smb_servers_of(host), inputs);
    ServerRow {
        name: host.name.clone(),
        meta: ServerMeta {
            row_id: host.id.clone(),
            protocol: ServerProtocol::Smb,
            address: host_address(host).unwrap_or_else(|| host.name.clone()),
            status: if mounted {
                ServerStatus::Connected
            } else {
                ServerStatus::FoundNearby
            },
            account: inputs.listed_as.get(&host.id).cloned(),
            saved: false,
            place: None,
            pinned: false,
        },
        last_connected_at: None,
        icon_id: SMB_SERVER_ICON_ID,
        opens: ServerOpens::ShareList(ShareListAddress::of_host(host)),
    }
}

/// A saved SMB host mDNS isn't seeing, as the host its share list opens: its address is
/// the only spelling anything has for it (`host`, or `host:port` off 445).
fn saved_host(server: &SavedServer) -> NetworkHost {
    let address = SmbServer::from_name(&server.address);
    NetworkHost {
        id: server.id.clone(),
        name: server.display_name.clone(),
        hostname: Some(address.host().to_string()),
        ip_address: None,
        port: address.port(),
        source: HostSource::Manual,
    }
}

/// Which of the three names the Name column shows.
///
/// ❗ A name a PERSON chose wins, then the Bonjour name mDNS found, then the stand-in
/// nobody chose. The top rank is a FACT the store publishes (`name_source`), ❌ never a
/// guess at the string's shape: an SMB host's label is either the way `statfs` spells
/// the server or the address typed into "Add server", and without the rank the friendly
/// name a person recognizes would vanish the moment they used the host.
fn display_name(server: &SavedServer, host: Option<&NetworkHost>) -> String {
    match (server.name_source, host) {
        (ServerNameSource::User, _) | (ServerNameSource::Fallback, None) => server.display_name.clone(),
        (ServerNameSource::Fallback, Some(host)) => host.name.clone(),
    }
}

/// How live a saved row is.
///
/// ❗ Off the VOLUME LIST and the MOUNT TABLE, ❌ never off `SavedPlace.connected`: that
/// flag is a snapshot from the moment the listing was built, while the volume list is
/// what the switcher's dot and the pane both read.
///
/// An SMB host is connected while a share from it is mounted (Cmdr's mount or the OS's,
/// by `SmbServer::is`: address and port). Otherwise a host saved without contact reads
/// "Not connected yet" even while mDNS sees it (#777), and mDNS seeing a DISCOVERED host
/// is the rest of the answer. ❌ Never the host's own manual entry: Cmdr injects every
/// typed-in host into the discovery list at startup, reachable or not, so that entry
/// says nothing about the network.
fn saved_status(
    server: &SavedServer,
    hosts: &[&NetworkHost],
    primary: Option<&NetworkHost>,
    inputs: &ServerRowInputs<'_>,
) -> ServerStatus {
    if server.protocol == ServerProtocol::Smb {
        let mut targets: Vec<SmbServer> = hosts.iter().flat_map(|host| smb_servers_of(host)).collect();
        targets.push(SmbServer::from_name(&server.address));
        if is_mounted(&targets, inputs) {
            return ServerStatus::Connected;
        }
        if server.awaiting_first_connect {
            return ServerStatus::NotConnectedYet;
        }
        return match primary.map(|host| host.source) {
            Some(HostSource::Discovered) => ServerStatus::FoundNearby,
            Some(HostSource::Manual) | None => ServerStatus::Saved,
        };
    }
    // An S3 account reads as its most urgent place: live if one is, else asking if one
    // asks. A one-place server has exactly one.
    server
        .places
        .iter()
        .map(|place| place_status(inputs.volume_states.get(&place.volume_id).copied()))
        .fold(ServerStatus::Saved, |best, status| {
            if urgency(status) < urgency(best) { status } else { best }
        })
}

/// Whether a share from one of `targets` is in the mount table.
fn is_mounted(targets: &[SmbServer], inputs: &ServerRowInputs<'_>) -> bool {
    inputs
        .smb_mounts
        .iter()
        .any(|mount| mount_is_from(mount, targets, inputs.hosts))
}

/// How live one place is, off its volume's connection state.
fn place_status(state: Option<ConnectionState>) -> ServerStatus {
    match state {
        Some(ConnectionState::Direct | ConnectionState::OsMount) => ServerStatus::Connected,
        Some(ConnectionState::NeedsSignIn) => ServerStatus::SignedOut,
        Some(ConnectionState::NeedsHostKeyApproval) => ServerStatus::WaitingForKey,
        // `Disconnected` says a backoff loop is running, which is a detail of HOW the
        // place gets back: to a person it's still one of their saved servers, and the
        // switcher's dot is where liveness is spelled out.
        Some(ConnectionState::Disconnected | ConnectionState::Saved) | None => ServerStatus::Saved,
    }
}

/// Most urgent first: a live session, then one asking something of the person, then
/// the rest.
fn urgency(status: ServerStatus) -> u8 {
    match status {
        ServerStatus::Connected => 0,
        ServerStatus::SignedOut | ServerStatus::WaitingForKey => 1,
        ServerStatus::Saved | ServerStatus::NotConnectedYet => 2,
        ServerStatus::FoundNearby => 3,
    }
}

/// The most useful spelling of where a discovered host lives, with its port when it
/// isn't 445: `localhost` alone would name another server on the same machine.
fn host_address(host: &NetworkHost) -> Option<String> {
    let address = host.ip_address.as_deref().or(host.hostname.as_deref())?;
    Some(match host.port {
        SMB_PORT => address.to_string(),
        port if address.contains(':') => format!("[{address}]:{port}"),
        port => format!("{address}:{port}"),
    })
}

fn icon_for(protocol: ServerProtocol) -> &'static str {
    match protocol {
        ServerProtocol::Smb => SMB_SERVER_ICON_ID,
        ServerProtocol::Sftp | ServerProtocol::Webdav | ServerProtocol::S3 | ServerProtocol::Nfs => SERVER_ICON_ID,
    }
}

fn user(username: Option<&str>) -> Option<ShareListAccount> {
    username
        .filter(|name| !name.is_empty())
        .map(|username| ShareListAccount::User {
            username: username.to_string(),
        })
}

#[cfg(test)]
#[path = "servers_listing_test.rs"]
mod servers_listing_test;
