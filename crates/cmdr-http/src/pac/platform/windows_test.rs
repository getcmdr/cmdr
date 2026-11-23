//! What only WinHTTP's runner does, against a PAC file on loopback: its typed answers and its
//! cancellation. The cache, budget, and timeout cells every PAC runner shares are `crate::pac`'s
//! `pac_test.rs`.

use std::time::Instant;

use super::*;
use crate::pac::test_server::pac_server;
use crate::pac::{ANSWER_BUDGET, OsPac};

/// What WinHTTP makes of a PAC that answers `answer` for every URL.
fn pac_answer(script: &'static str) -> Option<Vec<Entry>> {
    use crate::entries::Pac;
    let pac = pac_server(Some(script));
    OsPac::new(ANSWER_BUDGET).evaluate(
        &PacSource::Url(pac),
        &crate::pac::test_server::url("https://example.com"),
    )
}

fn socks(host: &str, port: u16) -> Entry {
    Entry::Socks {
        host: host.into(),
        port,
        credentials: None,
    }
}

/// A PAC that says SOCKS gets a SOCKS proxy, ❌ never direct (the older synchronous WinHTTP call
/// reported "no proxy" for it, which sent the request around the tunnel the PAC asked for).
#[test]
fn a_socks_answer_is_a_socks_entry() {
    assert_eq!(
        pac_answer(r#"function FindProxyForURL(url, host) { return "SOCKS s.test:1080"; }"#),
        Some(vec![socks("s.test", 1080)])
    );
}

/// The versioned keywords (`SOCKS5`, `SOCKS4`, Chrome's and Firefox's extensions) are WinHTTP's
/// to read: the Windows 10 22H2 builder's typed answer carries them, and the hosted runner's
/// Windows Server 2025 drops `SOCKS5` from the list (CI run 45158786958, 2026-11-23), as
/// CFNetwork drops both on macOS. Either way Cmdr goes where the OS's own clients go, and a
/// keyword it does read is never anything but SOCKS.
#[test]
fn a_versioned_socks_answer_is_socks_or_dropped_as_the_os_drops_it() {
    let five = pac_answer(r#"function FindProxyForURL(url, host) { return "SOCKS5 s.test:1081; DIRECT"; }"#);
    assert!(
        five == Some(vec![socks("s.test", 1081), Entry::Direct]) || five == Some(vec![Entry::Direct]),
        "{five:?}"
    );
    let four = pac_answer(r#"function FindProxyForURL(url, host) { return "SOCKS4 s.test:1082"; }"#);
    let read = Some(vec![Entry::Socks4 {
        host: "s.test".into(),
        port: 1082,
    }]);
    assert!(four == read || four == Some(vec![]), "{four:?}");
}

/// `HTTPS host:port` is a TLS proxy, ❌ never a plain HTTP one on its TLS port.
#[test]
fn an_https_answer_is_a_tls_proxy() {
    assert_eq!(
        pac_answer(r#"function FindProxyForURL(url, host) { return "HTTPS h.test:443"; }"#),
        Some(vec![Entry::Https {
            host: "h.test".into(),
            port: 443
        }])
    );
}

/// Every proxy the PAC lists comes through, in order, its trailing `DIRECT` too.
#[test]
fn a_list_keeps_its_order_and_its_trailing_direct() {
    let http = |host: &str, port| Entry::Http {
        host: host.into(),
        port,
        credentials: None,
    };
    assert_eq!(
        pac_answer(r#"function FindProxyForURL(url, host) { return "PROXY a.test:1; PROXY b.test:2; DIRECT"; }"#),
        Some(vec![http("a.test", 1), http("b.test", 2), Entry::Direct])
    );
}

/// A PAC server that accepts and never answers costs the evaluation its budget, then the
/// resolver's close cancels it.
#[test]
fn a_stalling_pac_server_is_given_up_on_within_the_budget() {
    let pac = pac_server(None);
    let budget = Duration::from_millis(500);
    let started = Instant::now();
    assert_eq!(execute(&PacSource::Url(pac), "https://example.com", budget), None);
    let took = started.elapsed();
    assert!(took >= budget && took < budget * 3, "took {took:?}");
}

/// Windows' settings never name an inline script, so it has no runner.
#[test]
fn an_inline_script_has_no_runner() {
    assert_eq!(
        execute(
            &PacSource::Script("function FindProxyForURL() {}".into()),
            "https://example.com",
            Duration::from_secs(1)
        ),
        None
    );
}
