//! A saved SMB entry names an address a dial can reach: a hostname or an IP, ❌ never a
//! Bonjour display name. A child of `manual_servers`, whose every read goes through
//! [`usable_entries`].
//!
//! ❗ "Sign in as…" on a discovered host once saved its display name (`SMB Test (Auth)`)
//! as the address, on 445, and the Servers list then dialed that literally
//! (`NT_STATUS_INVALID_PARAMETER`). The write side now saves [`dialable_server_of`] the
//! host, and the read side repairs or sets aside any entry an older build wrote that way.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Mutex;

use cmdr_fs::log_private::LogPrivate;
use log::{info, warn};

use crate::ignore_poison::IgnorePoison;
use crate::network::server_identity::{SmbServer, same_machine};
use crate::network::{HostSource, NetworkHost};

use super::{ManualServerEntry, generate_server_id};

/// Whether `address` can be dialed: an IP literal (a bracketed IPv6 one too), or a
/// hostname (ASCII letters, digits, `.`, `-`, `_`). A display name a person reads, with
/// its spaces and parentheses, is neither.
pub(crate) fn is_dialable(address: &str) -> bool {
    let bare = address.trim_start_matches('[').trim_end_matches(']');
    if bare.parse::<IpAddr>().is_ok() {
        return true;
    }
    !address.is_empty()
        && address
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// The server `host` is dialed as: its hostname, else its IP, on ITS port. `None` while
/// discovery hasn't resolved either, ❌ never its `name`: a Bonjour name is for people.
pub(crate) fn dialable_server_of(host: &NetworkHost) -> Option<SmbServer> {
    host.hostname
        .iter()
        .chain(host.ip_address.iter())
        .find(|address| is_dialable(address))
        .map(|address| SmbServer::new(address, host.port))
}

/// The server a discovery name (`host` or `host:port`) spells, when its host is an address;
/// `None` for a Bonjour display name.
pub(crate) fn dialable_name(name: &str) -> Option<SmbServer> {
    let server = SmbServer::from_name(name);
    is_dialable(server.host()).then_some(server)
}

/// The entries a consumer may use, off the `servers` the file holds. Each entry whose
/// address isn't dialable (see the module docs) is logged once, then becomes the
/// discovered host in `hosts` it names, by identity (`same_machine`), keeping its
/// account, or is skipped when nothing discovered goes by it. A repaired entry whose
/// server is already saved folds into that one. The next write persists the result.
pub(super) fn usable_entries(servers: Vec<ManualServerEntry>, hosts: &[NetworkHost]) -> Vec<ManualServerEntry> {
    let (mut usable, undialable): (Vec<_>, Vec<_>) = servers.into_iter().partition(|e| is_dialable(&e.address));
    for entry in undialable {
        let Some(server) = discovered_server_named(&entry.address, hosts) else {
            log_once(&entry, None);
            continue;
        };
        log_once(&entry, Some(&server));
        let saved = usable
            .iter_mut()
            .find(|e| SmbServer::new(&e.address, e.port).is(&server, hosts));
        match saved {
            Some(saved) => {
                if saved.username.is_none() {
                    saved.username = entry.username;
                }
            }
            None => usable.push(ManualServerEntry {
                id: generate_server_id(server.host(), server.port()),
                address: server.host().to_string(),
                port: server.port(),
                ..entry
            }),
        }
    }
    usable
}

/// The discovered host whose Bonjour name `name` is, as it's dialed.
fn discovered_server_named(name: &str, hosts: &[NetworkHost]) -> Option<SmbServer> {
    hosts
        .iter()
        .filter(|host| host.source == HostSource::Discovered && same_machine(name, &host.name, hosts))
        .find_map(dialable_server_of)
}

/// Logs an undialable entry once per session: the store is read on every Servers listing.
fn log_once(entry: &ManualServerEntry, repaired_as: Option<&SmbServer>) {
    static LOGGED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    if !LOGGED
        .lock_ignore_poison()
        .get_or_insert_with(HashSet::new)
        .insert(entry.id.clone())
    {
        return;
    }
    match repaired_as {
        Some(server) => info!(
            target: "network::manual_servers",
            "Saved SMB server {:?} isn't an address; using the discovered host it names, {:?}:{}",
            LogPrivate::host(&entry.address),
            LogPrivate::host(server.host()),
            server.port()
        ),
        None => warn!(
            target: "network::manual_servers",
            "Saved SMB server {:?} isn't an address and nothing discovered goes by it; not dialing it",
            LogPrivate::host(&entry.address)
        ),
    }
}

#[cfg(test)]
#[path = "manual_servers_dialable_test.rs"]
mod tests;
