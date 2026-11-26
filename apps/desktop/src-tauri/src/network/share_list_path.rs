//! Reading the path of a server's share list, `smb://<key>[:<port>]` on the `network`
//! volume, and finding the server it names.
//!
//! The writer is the frontend's (`apps/desktop/src/lib/servers/share-list-path.ts`, which
//! says why the key is a server's Bonjour name or typed address and never its IP); this is
//! the one Rust reader. Both read one case table, `share-list-path-cases.json` beside the
//! writer, so the escaping and the case and port folding can't drift apart.
//!
//! ❗ **Resolution reads only what's in memory**: discovery's hosts and the manual servers'
//! store. It never dials, so any reader of the volume may resolve a path.

use crate::network::{HostSource, NetworkHost, service_name_to_hostname};

/// SMB's own port, which a share-list path leaves unsaid.
const SMB_PORT: u16 = 445;

const SCHEME: &str = "smb://";

/// What a share-list path names: the server's key and its port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShareListAddress {
    /// The server's key, unescaped and folded ([`fold`]).
    pub key: String,
    pub port: u16,
}

impl ShareListAddress {
    /// The address `host`'s share list goes by: its Bonjour name when discovered, the
    /// address the person typed when manual, folded. The writer's rule, in Rust.
    pub(crate) fn of_host(host: &NetworkHost) -> Self {
        let name = match host.source {
            HostSource::Manual => host.hostname.as_deref().unwrap_or(&host.name),
            HostSource::Discovered => &host.name,
        };
        Self {
            key: fold(name),
            port: host.port,
        }
    }
}

/// Where a path on the `network` volume points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkPath {
    /// `smb://`, the Servers hub.
    Hub,
    /// `smb://<key>[:<port>]`, a server's share list.
    ShareList(ShareListAddress),
    /// `smb://<key>[:<port>]/<share>[/…]`: a share, or a path inside one. A share is
    /// mounted, ❌ never listed here.
    InShare(SharePath),
    /// Anything else: another scheme, or a spelling that doesn't read.
    Elsewhere,
}

/// A path at or under a share, as [`NetworkPath::of`] read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharePath {
    /// The server.
    pub server: ShareListAddress,
    /// Its share list's path, as this path spells it.
    pub share_list: String,
    /// The share's name, as the path spells it.
    pub share: String,
    /// Whether the path goes past the share itself.
    pub inside: bool,
}

impl NetworkPath {
    /// Reads `path` as the frontend spells it (`smb://testnas`) or as the app-path grammar
    /// does, which ends a scheme root in `/` (`smb://testnas/`, the hub `smb:///`).
    pub(crate) fn of(path: &str) -> Self {
        let Some(rest) = path.strip_prefix(SCHEME) else {
            return Self::Elsewhere;
        };
        if rest.is_empty() || rest == "/" {
            return Self::Hub;
        }
        let (authority, inside) = match rest.split_once('/') {
            Some((authority, "")) => (authority, None),
            Some((authority, inside)) => (authority, Some(inside)),
            None => (rest, None),
        };
        let Some(server) = read_authority(authority) else {
            return Self::Elsewhere;
        };
        let Some(inside) = inside else {
            return Self::ShareList(server);
        };
        let (share, rest) = match inside.split_once('/') {
            Some((share, rest)) => (share, Some(rest)),
            None => (inside, None),
        };
        if share.is_empty() {
            return Self::Elsewhere;
        }
        Self::InShare(SharePath {
            server,
            share_list: format!("{SCHEME}{authority}"),
            share: share.to_string(),
            inside: rest.is_some(),
        })
    }
}

/// The server `path` names, when it's a share list's path.
#[cfg(test)]
pub(crate) fn share_list_address(path: &str) -> Option<ShareListAddress> {
    match NetworkPath::of(path) {
        NetworkPath::ShareList(address) => Some(address),
        _ => None,
    }
}

/// Reads `key[:port]` or `[ipv6][:port]`, the writer's two spellings.
fn read_authority(authority: &str) -> Option<ShareListAddress> {
    let (key, port_text) = if let Some(bracketed) = authority.strip_prefix('[') {
        let (literal, after) = bracketed.split_once(']')?;
        let literal = literal.to_ascii_lowercase();
        if !is_ipv6_like(&literal) {
            return None;
        }
        let port_text = match after {
            "" => None,
            _ => Some(after.strip_prefix(':')?),
        };
        (literal, port_text)
    } else {
        let mut parts = authority.split(':');
        let raw_key = parts.next()?;
        let port_text = parts.next();
        if parts.next().is_some() {
            return None;
        }
        (unescape(raw_key)?, port_text)
    };
    let port = match port_text {
        None => SMB_PORT,
        Some(text) => read_port(text)?,
    };
    if key.is_empty() {
        return None;
    }
    Some(ShareListAddress { key: fold(&key), port })
}

/// The writer's IPv6 test: hex digits, dots, and at least two colons.
fn is_ipv6_like(text: &str) -> bool {
    text.matches(':').count() >= 2 && text.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
}

fn read_port(text: &str) -> Option<u16> {
    if text.is_empty() || text.len() > 5 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u16>().ok().filter(|port| *port != 0)
}

/// `text` with every `%XX` decoded, or `None` when it isn't one the writer could have
/// written: an escape that doesn't read or names no ASCII character, or a character the
/// writer always escapes standing bare.
fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '%' => {
                let hex: String = chars.by_ref().take(2).collect();
                if hex.len() != 2 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                let byte = u8::from_str_radix(&hex, 16).ok().filter(u8::is_ascii)?;
                out.push(char::from(byte));
            }
            '\\' | '[' | ']' => return None,
            _ => out.push(c),
        }
    }
    Some(out)
}

/// Lowercases and NFC-folds a key, the way every SMB server name is compared
/// (`network/CLAUDE.md`), with a final sigma read as a sigma: the writer lowercases with
/// JavaScript's `toLowerCase`, which writes `ς` at a word's end where Rust writes `σ`.
fn fold(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;

    text.nfc()
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ς' { 'σ' } else { c })
        .collect()
}

/// The servers a share-list path may resolve to, by how much each one is worth.
#[derive(Debug, Default, Clone)]
pub(crate) struct KnownHosts {
    /// Discovery's current evidence (`fresh_discovered_hosts`): the live browse's hosts,
    /// and the manual and E2E hosts it pins.
    pub fresh: Vec<NetworkHost>,
    /// The manual servers' store, as discovery would list them, for a launch where
    /// discovery hasn't loaded them yet.
    pub saved: Vec<NetworkHost>,
    /// A host for each server an SMB share is mounted from, made the way `..` out of the
    /// share makes one (`smb_share_parent::mounted_hosts`): a share macOS mounted by an
    /// address nothing else knows (a Tailscale IP) still has a share list to go up to.
    pub mounted: Vec<NetworkHost>,
    /// Every host discovery ever listed, stale ones included (`cached_discovered_hosts`).
    pub cached: Vec<NetworkHost>,
}

impl KnownHosts {
    /// The host a share list already resolved, by the id it listed under ([`resolve`]
    /// keeps the id, a stale host's too), in [`resolve`]'s order.
    pub(crate) fn by_id(&self, id: &str) -> Option<&NetworkHost> {
        self.fresh
            .iter()
            .chain(&self.saved)
            .chain(&self.mounted)
            .chain(&self.cached)
            .find(|host| host.id == id)
    }
}

/// The server `address` names among `hosts`, or `None` when nothing known goes by it.
///
/// Fresh evidence first, a discovered host before a manual one (the hub's own pick, so
/// the pane lists under the host id the hub caches under); then the manual store; then a
/// server a share is mounted from, which is all that knows a share macOS mounted by an
/// address Cmdr never saw (`..` out of that share leads here). A host
/// only the display cache still lists comes last, and ❗ with its IP dropped: its name ↔
/// address pairing is stale (`network/CLAUDE.md`, identity reads fresh evidence only), so
/// the dial resolves its `.local` name afresh rather than trusting an address that may be
/// another machine's by now.
pub(crate) fn resolve(address: &ShareListAddress, hosts: &KnownHosts) -> Option<NetworkHost> {
    let named = |host: &&NetworkHost| ShareListAddress::of_host(host) == *address;
    let fresh = |source: HostSource| hosts.fresh.iter().filter(named).find(|host| host.source == source);
    if let Some(host) = fresh(HostSource::Discovered).or_else(|| fresh(HostSource::Manual)) {
        return Some(host.clone());
    }
    if let Some(host) = hosts.saved.iter().chain(&hosts.mounted).find(named) {
        return Some(host.clone());
    }
    hosts.cached.iter().find(named).map(|stale| NetworkHost {
        ip_address: None,
        hostname: Some(
            stale
                .hostname
                .clone()
                .unwrap_or_else(|| service_name_to_hostname(&stale.name)),
        ),
        ..stale.clone()
    })
}

#[cfg(test)]
#[path = "share_list_path_test.rs"]
mod tests;
