//! Servers behind mounted shares: Cmdr saves the SERVER of every SMB share the kernel
//! has mounted (Finder's, `mount_smbfs`'s, anyone's), so it shows in the Servers list
//! and stays there after the share unmounts. A child of `manual_servers` so it shares
//! that store's lock and file helpers.
//!
//! ❗ **Only the server, ❌ never a share**: only Cmdr's own mounts save a share
//! (`smb_saved_shares.rs`), and a share the person never opened in Cmdr is no history.
//!
//! ❗ **Nothing here touches the network or a mount**: the mount table
//! (`volumes::smb_mounts`, ❌ never a `statfs` per mount, which hangs on a dead share),
//! the two stores, and discovery's memory. The saved entry is `saved_from_mount`, which
//! keeps it out of the discovery list and so out of anything that lists shares unasked
//! (#324), until a mount through Cmdr makes it ordinary (`note_first_connect`).
//!
//! ❗ **Keyed by the mount's own spelling** (`statfs`'s, port included), ❌ never by the
//! address discovery resolves it to: that flips between an IP and a `.local` name with
//! discovery timing, so launches would save one NAS again and again. An mDNS service
//! name (`Testnas._smb._tcp.local`) is keyed by its `.local` hostname, which the
//! Servers list pairs with the discovered host of that name.

use std::path::Path;

use log::info;

use super::{
    MANUAL_SERVERS_FILENAME, ManualServerEntry, generate_server_id, get_store_lock, read_store_from_path,
    write_store_to_path,
};
use crate::network::NetworkHost;
use crate::network::known_shares::KnownNetworkShare;
use crate::network::server_identity::SmbServer;
use crate::volumes::SmbMountInfo;

/// SMB's port when a share-history row names none.
const SMB_PORT: u16 = 445;

/// Saves the server of every SMB share mounted now that no store knows yet, and tells
/// the Servers list when one was. Runs at launch and on every mount event.
///
/// ❗ Blocking (two small file reads, maybe a write): call it off the main thread.
pub fn remember_mounted_servers() {
    let Some(path) = crate::config::standalone_app_data_dir().map(|dir| dir.join(MANUAL_SERVERS_FILENAME)) else {
        return;
    };
    let Some(mounts) = crate::volumes::smb_mounts() else {
        return;
    };
    let mounts: Vec<SmbMountInfo> = mounts.into_iter().map(|(_, info)| info).collect();
    if mounts.is_empty() {
        return;
    }
    let to_save = servers_to_save(
        &mounts,
        &read_store_from_path(&path).servers,
        &crate::network::known_shares::get_all_known_shares(),
        &crate::network::fresh_discovered_hosts(),
    );
    if to_save.is_empty() {
        return;
    }
    let saved = save_at_path(&path, &to_save);
    if saved > 0 {
        info!("Saved {saved} SMB server(s) a share was mounted from, so they stay in the Servers list");
        crate::volume_broadcast::emit_volumes_changed();
    }
}

/// The servers `mounts` come from that neither store lists yet, each once, in mount
/// order. Pure.
pub(super) fn servers_to_save(
    mounts: &[SmbMountInfo],
    entries: &[ManualServerEntry],
    known: &[KnownNetworkShare],
    hosts: &[NetworkHost],
) -> Vec<SmbServer> {
    let mut to_save: Vec<SmbServer> = Vec::new();
    for info in mounts {
        let server = keyed_server(info);
        let id = generate_server_id(server.host(), server.port());
        let saved = entries
            .iter()
            .any(|entry| entry.id == id || SmbServer::new(&entry.address, entry.port).is(&server, hosts));
        let in_history = known
            .iter()
            .any(|row| history_names(row).iter().any(|named| named.is(&server, hosts)));
        let queued = to_save.iter().any(|queued| queued.is(&server, hosts));
        if !saved && !in_history && !queued {
            to_save.push(server);
        }
    }
    to_save
}

/// The server a mount is from, spelled the way it's saved: the mount's own spelling,
/// except an mDNS service name, which is no hostname and becomes its `.local` one.
fn keyed_server(info: &SmbMountInfo) -> SmbServer {
    if crate::network::smb_server_address::is_service_name(&info.server) {
        let instance = info.server.split("._").next().unwrap_or(&info.server);
        return SmbServer::new(&crate::network::service_name_to_hostname(instance), info.port);
    }
    SmbServer::new(&info.server, info.port)
}

/// The servers a share-history row names: where its mount dialed, on its port, and the
/// name it was filed under (a discovery name, `host:port` off 445).
fn history_names(row: &KnownNetworkShare) -> Vec<SmbServer> {
    let mut names = vec![SmbServer::from_name(&row.server_name)];
    if let Some(address) = &row.address {
        names.push(SmbServer::new(address, row.port.unwrap_or(SMB_PORT)));
    }
    names
}

/// Writes an entry for each of `servers` the store at `path` doesn't hold yet (by id,
/// under the lock), and answers how many it wrote.
pub(super) fn save_at_path(path: &Path, servers: &[SmbServer]) -> usize {
    let _guard = get_store_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut store = read_store_from_path(path);
    let mut wrote = 0;
    for server in servers {
        let id = generate_server_id(server.host(), server.port());
        if store.servers.iter().any(|entry| entry.id == id) {
            continue;
        }
        store.servers.push(ManualServerEntry {
            id,
            display_name: String::new(),
            address: server.host().to_string(),
            port: server.port(),
            added_at: crate::timestamps::now_rfc3339(),
            username: None,
            awaiting_first_connect: false,
            saved_from_mount: true,
        });
        wrote += 1;
    }
    if wrote > 0 {
        write_store_to_path(path, &store);
    }
    wrote
}

#[cfg(test)]
#[path = "manual_servers_mounted_test.rs"]
mod tests;
