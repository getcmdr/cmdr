//! Windows' Internet settings (the ones `inetcpl.cpl`'s "LAN settings" and Settings > Network >
//! Proxy edit, read by `WinHttpGetIEProxyConfigForCurrentUser`), and the proxy-list text WinHTTP
//! answers a PAC with, as [`Entry`] lists. Pure text in, entries out, so every rule runs in every
//! OS's tests; `windows.rs` does the reading.
//!
//! The order is the one WinINet and Chromium give these settings: "Automatically detect
//! settings" (WPAD) first, then "Use setup script" (a PAC URL), then the manual proxy and its
//! bypass list. A PAC that can't run falls through to the next, which
//! `crate::entries::first_route` already does for any list.
#![cfg_attr(
    any(target_os = "macos", target_os = "linux"),
    allow(dead_code, reason = "Windows' reader; on macOS and Linux only its tests use it")
)]

use reqwest::Url;

use crate::entries::Entry;

/// The current user's Internet settings, as Windows hands them over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IeSettings {
    /// "Automatically detect settings": WPAD.
    pub(crate) auto_detect: bool,
    /// "Use setup script": the PAC file's URL.
    pub(crate) auto_config_url: Option<String>,
    /// The manual proxy: `host:port` for every scheme, or per scheme,
    /// `http=host:port;https=host:port;socks=host:port`.
    pub(crate) proxy: Option<String>,
    /// The manual proxy's bypass list ("Don't use the proxy server for…"), `;`-separated, with
    /// `*` wildcards and `<local>` for every dotless host name.
    pub(crate) bypass: Option<String>,
}

/// The ordered list `settings` give for `url`.
pub(crate) fn entries_for(settings: &IeSettings, url: &Url) -> Vec<Entry> {
    let mut entries = Vec::new();
    if settings.auto_detect {
        entries.push(Entry::AutoDetect);
    }
    if let Some(pac) = non_empty(settings.auto_config_url.as_deref()) {
        entries.push(Entry::AutoConfigUrl(pac.to_string()));
    }
    if let Some(proxy) = non_empty(settings.proxy.as_deref()) {
        let bypassed = settings.bypass.as_deref().is_some_and(|list| bypasses(list, url));
        let manual = (!bypassed).then(|| manual_proxy(proxy, url.scheme())).flatten();
        entries.push(manual.unwrap_or(Entry::Direct));
    }
    entries
}

/// A PAC's answer as WinHTTP spells it (`WINHTTP_PROXY_INFO::lpszProxy`): `host:port` entries
/// separated by `;` or whitespace, each tried in order, `DIRECT` where the PAC said so.
pub(crate) fn proxy_list_entries(list: &str) -> Vec<Entry> {
    tokens(list)
        .map(|token| {
            if token.eq_ignore_ascii_case("DIRECT") {
                return Entry::Direct;
            }
            // A `scheme=` prefix has no place in a PAC answer; read past one rather than guess.
            let value = token.split_once('=').map_or(token, |(_, value)| value);
            proxy_entry(value, ProxyKind::Http)
        })
        .collect()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn tokens(list: &str) -> impl Iterator<Item = &str> {
    list.split(|c: char| c == ';' || c.is_whitespace())
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyKind {
    Http,
    Socks,
}

/// The manual proxy for a URL of `scheme`: that scheme's own `scheme=` entry, else an entry for
/// every scheme, else the `socks=` one, else none (direct), as WinINet picks.
fn manual_proxy(proxy: &str, scheme: &str) -> Option<Entry> {
    let scheme = match scheme {
        "ws" => "http",
        "wss" => "https",
        other => other,
    };
    let mut every_scheme = None;
    let mut socks = None;
    for token in tokens(proxy) {
        match token.split_once('=') {
            Some((key, value)) if key.trim().eq_ignore_ascii_case(scheme) => {
                return Some(proxy_entry(value.trim(), ProxyKind::Http));
            }
            Some((key, value)) if key.trim().eq_ignore_ascii_case("socks") => {
                socks.get_or_insert(proxy_entry(value.trim(), ProxyKind::Socks));
            }
            Some(_) => {}
            None => {
                every_scheme.get_or_insert(proxy_entry(token, ProxyKind::Http));
            }
        }
    }
    every_scheme.or(socks)
}

/// One `[scheme://]host[:port]` proxy. A `socks…://` prefix makes it a SOCKS proxy whatever slot
/// it sits in; the port defaults to the kind's own (80, 1080).
fn proxy_entry(value: &str, kind: ProxyKind) -> Entry {
    let (kind, address) = match value.split_once("://") {
        Some((scheme, rest)) if scheme.to_ascii_lowercase().starts_with("socks") => (ProxyKind::Socks, rest),
        Some((_, rest)) => (kind, rest),
        None => (kind, value),
    };
    let address = address.trim_end_matches('/');
    let default_port = match kind {
        ProxyKind::Http => 80,
        ProxyKind::Socks => 1080,
    };
    let (host, port) = split_host_port(address);
    let port = port.unwrap_or(default_port);
    let host = host.to_string();
    match kind {
        ProxyKind::Http => Entry::Http {
            host,
            port,
            credentials: None,
        },
        ProxyKind::Socks => Entry::Socks {
            host,
            port,
            credentials: None,
        },
    }
}

/// `host[:port]`, an IPv6 host in brackets (`[::1]:8080`), the brackets dropped.
fn split_host_port(address: &str) -> (&str, Option<u16>) {
    if let Some(rest) = address.strip_prefix('[') {
        let (host, after) = rest.split_once(']').unwrap_or((rest, ""));
        let port = after.strip_prefix(':').and_then(|port| port.parse().ok());
        return (host, port);
    }
    match address.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse() {
            Ok(port) => (host, Some(port)),
            Err(_) => (address, None),
        },
        _ => (address, None),
    }
}

/// Whether the bypass `list` sends `url` direct. Each `;`-separated entry is a host pattern with
/// `*` wildcards (`*.corp.test`, `10.*`), optionally with a `scheme://` it applies to and a `:port`
/// it must match, or `<local>`: any host name without a dot. Case-insensitive, as WinINet is.
fn bypasses(list: &str, url: &Url) -> bool {
    let Some(host) = url.host_str() else { return false };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let port = url.port_or_known_default();
    tokens(list).any(|entry| {
        let entry = entry.to_ascii_lowercase();
        if entry == "<local>" {
            return !host.contains('.') && !host.contains(':');
        }
        if entry.starts_with('<') {
            // `<-loopback>` and friends: loopback goes direct before the system is asked.
            return false;
        }
        let pattern = match entry.split_once("://") {
            Some((scheme, rest)) if scheme == url.scheme() => rest,
            Some(_) => return false,
            None => entry.as_str(),
        };
        match split_host_port(pattern) {
            (pattern_host, Some(pattern_port)) => port == Some(pattern_port) && glob(pattern_host, &host),
            (pattern_host, None) => glob(pattern_host, &host),
        }
    })
}

/// `*`-wildcard match of the whole of `text` (both already lowercased).
fn glob(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

#[cfg(test)]
#[path = "ie_settings_test.rs"]
mod tests;
