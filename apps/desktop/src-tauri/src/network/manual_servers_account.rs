//! "Sign in as…": the account an SMB host is used with, written into the manual store.
//! A child of `manual_servers` so it shares that store's lock and file helpers.

use std::path::Path;

use tauri::{AppHandle, Runtime};

use super::{
    ManualServerEntry, create_network_host, generate_server_id, get_store_lock, get_store_path, read_store_from_path,
    typed_account, write_store_to_path,
};
use crate::network::server_identity::SmbServer;
use crate::network::{NetworkHost, on_host_found};

/// Sets the account `server` is used with, the same preference a typed username is
/// (it prefills the sign-in and keeps the listing off guest), protected by
/// `STORE_LOCK`. `None` clears it: the person chose guest. Answers the entry as stored.
///
/// ❗ Found by [`SmbServer::is`], the way [`typed_username`](super::typed_username) reads it, and the name
/// stays. A host nobody typed in is saved with no name, so the preference has
/// somewhere to live: the person just told Cmdr how they use this server. Clearing
/// an account a host never had saves nothing (`None`), and so does a `server` whose host
/// isn't an address a dial can reach (`dialable::is_dialable`).
pub(super) fn set_account_at_path(
    path: &Path,
    server: &SmbServer,
    hosts: &[NetworkHost],
    username: Option<&str>,
) -> Option<ManualServerEntry> {
    if !super::dialable::is_dialable(server.host()) {
        return None;
    }
    let _guard = get_store_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut store = read_store_from_path(path);
    let entry = if let Some(existing) = store
        .servers
        .iter_mut()
        .find(|s| SmbServer::new(&s.address, s.port).is(server, hosts))
    {
        existing.username = typed_account(username);
        existing.clone()
    } else {
        typed_account(username)?;
        let entry = ManualServerEntry {
            id: generate_server_id(server.host(), server.port()),
            display_name: String::new(),
            address: server.host().to_string(),
            port: server.port(),
            added_at: crate::timestamps::now_rfc3339(),
            username: typed_account(username),
            awaiting_first_connect: false,
            saved_from_mount: false,
        };
        store.servers.push(entry.clone());
        entry
    };
    write_store_to_path(path, &store);
    Some(entry)
}

/// Sets the account `server` is used with ("Sign in as…"). See [`set_account_at_path`].
/// Answers whether the store could be written; `false` for a `server` that isn't an address.
pub fn set_account<R: Runtime>(server: &SmbServer, username: Option<&str>, app_handle: &AppHandle<R>) -> bool {
    if !super::dialable::is_dialable(server.host()) {
        return false;
    }
    let Some(path) = get_store_path(app_handle) else {
        return false;
    };
    let hosts = crate::network::fresh_discovered_hosts();
    let was_saved = read_store_from_path(&path)
        .servers
        .iter()
        .any(|s| SmbServer::new(&s.address, s.port).is(server, &hosts));
    let Some(entry) = set_account_at_path(&path, server, &hosts, username) else {
        return true;
    };
    if !was_saved {
        on_host_found(create_network_host(&entry.address, entry.port), app_handle);
    }
    true
}
