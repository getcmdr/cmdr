//! The share-list path reader and the host resolver.
//!
//! The case table is the frontend writer's (`share-list-path-cases.json`): every path it
//! writes must read back here as the server it was written for, or a pane listing it
//! resolves another server, or none.

use super::*;

#[derive(serde::Deserialize)]
struct Cases {
    hosts: Vec<HostCase>,
    spellings: Vec<SpellingCase>,
    #[serde(rename = "notShareLists")]
    not_share_lists: Vec<PathCase>,
}

#[derive(serde::Deserialize)]
struct HostCase {
    why: String,
    host: NetworkHost,
    path: String,
    key: String,
    port: u16,
}

#[derive(serde::Deserialize)]
struct SpellingCase {
    why: String,
    path: String,
    key: String,
    port: u16,
}

#[derive(serde::Deserialize)]
struct PathCase {
    why: String,
    path: String,
}

fn cases() -> Cases {
    serde_json::from_str(include_str!("../../../src/lib/servers/share-list-path-cases.json"))
        .expect("the case table parses")
}

fn address(key: &str, port: u16) -> ShareListAddress {
    ShareListAddress {
        key: key.to_string(),
        port,
    }
}

/// ❗ Every path the frontend writes reads back as the server it was written for, with
/// the same key the resolver derives from that server.
#[test]
fn every_written_path_reads_back_as_its_server() {
    for case in cases().hosts {
        let read = share_list_address(&case.path);
        assert_eq!(read, Some(address(&case.key, case.port)), "{}", case.why);
        assert_eq!(
            ShareListAddress::of_host(&case.host),
            address(&case.key, case.port),
            "{}",
            case.why
        );
    }
}

/// ❗ The Rust writer writes every path the frontend's writer does, byte for byte: a
/// Servers row's path is what `..` from its share list selects the row by.
#[test]
fn the_rust_writer_writes_what_the_frontend_writer_writes() {
    for case in cases().hosts {
        assert_eq!(share_list_path(&case.host), case.path, "{}", case.why);
    }
}

/// JavaScript writes a word-final `Σ` as `ς`, and so does the Rust writer: the path is
/// the frontend's to the byte, while the reader folds both to one key.
#[test]
fn the_writer_lowercases_a_final_sigma_the_way_javascript_does() {
    assert_eq!(share_list_path(&discovered("ΑΘΗΝΑΣ", 445)), "smb://αθηνας");
}

#[test]
fn other_spellings_of_a_server_fold_to_its_key() {
    for case in cases().spellings {
        assert_eq!(
            share_list_address(&case.path),
            Some(address(&case.key, case.port)),
            "{}",
            case.why
        );
    }
}

#[test]
fn a_path_that_is_no_share_list_reads_as_none() {
    for case in cases().not_share_lists {
        assert_eq!(share_list_address(&case.path), None, "{}", case.why);
    }
}

/// The volume's own paths: the Servers list, a share list, and a share inside it.
#[test]
fn a_network_path_says_what_it_points_at() {
    assert_eq!(NetworkPath::of("smb://"), NetworkPath::ServersList);
    assert_eq!(
        NetworkPath::of("smb://testnas:4450"),
        NetworkPath::ShareList(address("testnas", 4450))
    );
    assert_eq!(
        NetworkPath::of("smb://TestNAS/archive"),
        NetworkPath::InShare(SharePath {
            server: address("testnas", 445),
            share_list: "smb://TestNAS".to_string(),
            share: "archive".to_string(),
            inside: false,
        })
    );
    assert_eq!(
        NetworkPath::of("smb://testnas/archive/photos"),
        NetworkPath::InShare(SharePath {
            server: address("testnas", 445),
            share_list: "smb://testnas".to_string(),
            share: "archive".to_string(),
            inside: true,
        })
    );
    assert_eq!(NetworkPath::of("/Volumes/archive"), NetworkPath::Elsewhere);
}

/// An accented name arrives composed from mDNS and maybe decomposed in a typed path:
/// one server either way.
#[test]
fn a_key_folds_unicode_normalization() {
    assert_eq!(
        share_list_address("smb://re\u{301}sz nas"),
        Some(address("r\u{e9}sz nas", 445))
    );
}

/// JavaScript lowercases a word-final `Σ` to `ς`, Rust to `σ`: one server either way.
#[test]
fn a_final_sigma_is_one_key_in_both_languages() {
    let host = discovered("ΑΘΗΝΑΣ", 445);
    assert_eq!(
        share_list_address("smb://αθηνας"),
        Some(ShareListAddress::of_host(&host))
    );
}

fn discovered(name: &str, port: u16) -> NetworkHost {
    NetworkHost {
        id: format!("id-{name}"),
        name: name.to_string(),
        hostname: Some("testnas.local".to_string()),
        ip_address: Some("192.168.50.20".to_string()),
        port,
        source: HostSource::Discovered,
    }
}

fn manual(address: &str, port: u16) -> NetworkHost {
    crate::network::manual_servers::create_network_host(address, port)
}

/// A discovered server resolves by its Bonjour name, case folded.
#[test]
fn a_discovered_server_resolves_by_its_name() {
    let hosts = KnownHosts {
        fresh: vec![discovered("Other NAS", 445), discovered("Testnas", 445)],
        ..KnownHosts::default()
    };

    let host = resolve(&address("testnas", 445), &hosts).expect("the server is known");

    assert_eq!(host.id, "id-Testnas");
    assert_eq!(
        host.ip_address.as_deref(),
        Some("192.168.50.20"),
        "fresh evidence keeps its IP"
    );
}

/// The port is part of the server: one machine can run two SMB servers.
#[test]
fn the_port_picks_between_two_servers_of_one_name() {
    let hosts = KnownHosts {
        fresh: vec![discovered("Testnas", 445), discovered("Testnas", 4450)],
        ..KnownHosts::default()
    };

    let host = resolve(&address("testnas", 4450), &hosts).expect("the second server is known");

    assert_eq!(host.port, 4450);
    assert_eq!(resolve(&address("testnas", 4451), &hosts).map(|h| h.port), None);
}

/// A manual server mDNS isn't seeing resolves from the manual store, by the typed
/// address, IPv6 included.
#[test]
fn a_manual_server_resolves_from_the_store_by_its_typed_address() {
    let hosts = KnownHosts {
        saved: vec![manual("TestNAS.local", 445), manual("fe80::1", 4450)],
        ..KnownHosts::default()
    };

    let named = resolve(&address("testnas.local", 445), &hosts).expect("the typed name resolves");
    assert_eq!(named.hostname.as_deref(), Some("TestNAS.local"));

    let ipv6 = share_list_address("smb://[fe80::1]:4450").expect("an IPv6 share list reads");
    let ipv6 = resolve(&ipv6, &hosts).expect("the IPv6 server resolves");
    assert_eq!(ipv6.ip_address.as_deref(), Some("fe80::1"));
    assert_eq!(ipv6.port, 4450);
}

/// ❗ One server seen by discovery AND typed by hand lists under the discovered host,
/// which is the row the Servers list shows and the share cache files it under.
#[test]
fn a_discovered_host_wins_over_a_manual_one_of_the_same_key() {
    let mut nas = discovered("TestNAS.local", 445);
    nas.id = "discovered-testnas".to_string();
    let hosts = KnownHosts {
        fresh: vec![manual("testnas.local", 445), nas],
        ..KnownHosts::default()
    };

    let host = resolve(&address("testnas.local", 445), &hosts).expect("the server is known");

    assert_eq!(host.id, "discovered-testnas");
}

/// ❗ A host only the display cache still lists resolves without its IP: the pairing is
/// stale, and an IP DHCP handed to another machine must not get the dial.
#[test]
fn a_stale_host_resolves_without_its_ip() {
    let mut stale = discovered("Testnas", 445);
    stale.hostname = None;
    let hosts = KnownHosts {
        cached: vec![stale],
        ..KnownHosts::default()
    };

    let host = resolve(&address("testnas", 445), &hosts).expect("the stale server still resolves");

    assert_eq!(host.ip_address, None);
    assert_eq!(host.hostname.as_deref(), Some("testnas.local"));
}

/// ❗ The app-path grammar takes every written path as ONE scheme root (it adds the
/// trailing `/` a scheme root carries), and the reader reads that spelling back as the
/// same server. A share under it is a name of its own.
#[test]
fn the_app_path_grammar_keeps_every_written_authority_whole() {
    use cmdr_fs::app_path::{AppPathBuf, PathFlavor};
    for case in cases().hosts {
        let parsed = AppPathBuf::parse(&case.path, PathFlavor::Posix).expect("a written path is an app path");
        assert_eq!(parsed.as_str(), format!("{}/", case.path), "{}", case.why);
        assert_eq!(
            share_list_address(parsed.as_str()),
            Some(address(&case.key, case.port)),
            "{}",
            case.why
        );
        let share = AppPathBuf::parse(&format!("{}/archive", case.path), PathFlavor::Posix).expect("a share path");
        assert_eq!(share.as_str(), format!("{}/archive", case.path), "{}", case.why);
    }
    assert_eq!(
        NetworkPath::of(
            AppPathBuf::parse("smb://", PathFlavor::Posix)
                .expect("the Servers list")
                .as_str()
        ),
        NetworkPath::ServersList
    );
}

#[test]
fn an_unknown_server_resolves_to_nothing() {
    let hosts = KnownHosts {
        fresh: vec![discovered("Testnas", 445)],
        ..KnownHosts::default()
    };
    assert!(resolve(&address("nobody", 445), &hosts).is_none());
}
