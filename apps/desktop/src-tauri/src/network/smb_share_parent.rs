//! "Show “..” in the top folder" for an SMB share: in a share's top folder, `..`
//! opens the server's list of shares (#1238). This module answers the two questions
//! the UI has about a share, both by volume id: where `..` leads (which server, under
//! what name, back onto which share), and what the share's own switch is set to.
//!
//! ❗ **One default plus per-share overrides, and an override is stored only while it
//! differs from the default.** The default is the frontend setting
//! `network.showShareParentRow`, which the caller hands in on every write, so flipping
//! a share back to match the default clears its entry and the row menu's plain
//! checkbox stays honest without a "use default" state. A default flipped later keeps
//! every override: each one is a choice someone made about that share.
//!
//! Overrides live in `known-shares.json` (`known_shares::parent_row_overrides`), keyed
//! by server + share and matched by `server_identity::same_machine` like the
//! direct-connection opt-outs, ❌ never by account: a share macOS mounted has no saved
//! row, and `statfs` spells one server several ways.
//!
//! ❗ **Every read here is memory or the kernel's cached mount table, ❌ never a `statfs`
//! on the share.** A top-folder listing asks it on its way out (`listing_host::events`),
//! so a wait on the server here would hold that listing, for seconds on a quiet server.

use crate::network::known_shares;
use crate::network::manual_servers::{create_network_host, typed_name};
use crate::network::server_identity::{SmbServer, smb_server, smb_servers_of};
use crate::network::share_choices::{choose_parent_row, parent_row_override};
use crate::network::smb_server_address::{ServerAddress, friendly_server_name, resolve_server_address};
use crate::network::{HostSource, NetworkHost, service_name_to_hostname};
use crate::volumes::{SmbMountInfo, smb_mount_info_at};
use serde::{Deserialize, Serialize};

/// Where `..` in a share's top folder leads, and the share's own switch.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareParent {
    /// The server, as its share list opens it.
    pub host: NetworkHost,
    /// What to call the server in words: the name a person typed for it, else the
    /// name it goes by, with its port off 445. The same name the share's own label uses.
    pub server_label: String,
    /// The share as the mount spells it, for the cursor to land back on.
    pub share_name: String,
    /// The share's own choice, or `None` while it follows the default.
    pub show_parent_row: Option<bool>,
}

/// Where `..` leads out of the share behind `volume_id`, or `None` when no SMB share
/// is behind it: no volume under that id, or no SMB mount at its root YET (a saved
/// share whose mount is still on its way). A later top-folder listing asks again.
pub(crate) fn share_parent_of(volume_id: &str) -> Option<ShareParent> {
    share_parent_with(volume_id, smb_mount_info_at)
}

fn share_parent_with(volume_id: &str, read: impl FnOnce(&str) -> Option<SmbMountInfo>) -> Option<ShareParent> {
    let info = share_at_root(volume_id, read)?;
    // Discovery is read BEFORE the store's lock is taken, so the two never nest.
    let hosts = crate::network::fresh_discovered_hosts();
    let show_parent_row = known_shares::read_parent_row_overrides(|overrides| {
        parent_row_override(overrides, &info.server, &info.share, &hosts)
    });
    Some(ShareParent {
        host: host_for_mount(&info, &hosts),
        server_label: server_label(&info),
        share_name: info.share,
        show_parent_row,
    })
}

/// Saves the share's switch, given the default it's measured against. Answers `false`
/// when no SMB share is behind `volume_id`, and then nothing was saved.
pub(crate) fn set_parent_row_of(volume_id: &str, show: bool, default_show: bool) -> bool {
    set_parent_row_with(volume_id, show, default_show, smb_mount_info_at)
}

fn set_parent_row_with(
    volume_id: &str,
    show: bool,
    default_show: bool,
    read: impl FnOnce(&str) -> Option<SmbMountInfo>,
) -> bool {
    let Some(info) = share_at_root(volume_id, read) else {
        return false;
    };
    let hosts = crate::network::fresh_discovered_hosts();
    known_shares::update_parent_row_overrides(|overrides| {
        choose_parent_row(overrides, &info.server, &info.share, show, default_show, &hosts);
    });
    true
}

/// The SMB share mounted at the root of the volume `volume_id` names, as `read` finds
/// it there (the kernel's cached mount table in the app). A direct share is read off
/// its mount too: its OS mount stays up underneath the session, and spells the server
/// and share the way the auto upgrade sees them.
fn share_at_root(volume_id: &str, read: impl FnOnce(&str) -> Option<SmbMountInfo>) -> Option<SmbMountInfo> {
    let volume = crate::file_system::volume::manager::get_volume_manager().get(volume_id)?;
    // A server URL (an SFTP or WebDAV place) has no native spelling, so no share is mounted there.
    let root = volume.root().to_native().ok()?;
    read(&root.to_string_lossy())
}

/// The server a mount is from, as the share list opens it.
///
/// A host discovery knows is preferred, a resolved one first, so the list reads the
/// same cached shares and sign-in the servers hub does (both key on the host). With
/// none, the host is made from the mount's own server the way "Add server" makes one,
/// so `..` works with discovery off too.
fn host_for_mount(info: &SmbMountInfo, hosts: &[NetworkHost]) -> NetworkHost {
    let mounted = SmbServer::new(&info.server, info.port);
    let serves = |host: &&NetworkHost| smb_servers_of(host).iter().any(|server| server.is(&mounted, hosts));
    let rank = |host: &NetworkHost| match (host.hostname.is_some(), host.source) {
        (true, HostSource::Discovered) => 0,
        (true, HostSource::Manual) => 1,
        (false, _) => 2,
    };
    let mut matching: Vec<&NetworkHost> = hosts.iter().filter(serves).collect();
    matching.sort_by_key(|host| rank(host));
    matching
        .first()
        .map(|host| (*host).clone())
        .unwrap_or_else(|| made_host(info))
}

/// A host for a mount discovery doesn't know: dialable by the address the mount's
/// server resolves to, or, for an mDNS service name nothing has resolved yet, by its
/// `.local` hostname.
fn made_host(info: &SmbMountInfo) -> NetworkHost {
    let address = match resolve_server_address(&info.server) {
        ServerAddress::Connectable(address) => address,
        ServerAddress::UndiscoveredService => service_name_to_hostname(&friendly_server_name(&info.server)),
    };
    create_network_host(&address, info.port)
}

/// The server's name in words, the way `smb_share_label::mounted` names it.
fn server_label(info: &SmbMountInfo) -> String {
    typed_name(&[SmbServer::new(&info.server, info.port)])
        .unwrap_or_else(|| smb_server(&friendly_server_name(&info.server), info.port))
}

#[cfg(test)]
#[path = "smb_share_parent_test.rs"]
mod tests;
