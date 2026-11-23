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

/// This Windows build (`10.0.<build>`), from `ver`: WinHTTP's PAC keywords differ by build.
fn windows_build() -> u32 {
    let out = std::process::Command::new("cmd")
        .args(["/c", "ver"])
        .output()
        .expect("`ver` runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text
        .rsplit("Version ")
        .next()
        .and_then(|rest| rest.split(']').next())
        .expect("`ver` prints `[Version 10.0.<build>.<rev>]`");
    version
        .split('.')
        .nth(2)
        .and_then(|build| build.parse().ok())
        .expect("a numeric build")
}

/// The first build whose WinHTTP drops a PAC's versioned `SOCKS5` and `SOCKS4` entries: 26100,
/// Windows Server 2025 (the hosted runner, CI runs 45158786958 and 45204801821, 2026-11-23) and,
/// by the same build number, Windows 11 24H2. Windows 10 22H2 (19045, the builder) reads both.
const DROPS_VERSIONED_SOCKS: u32 = 26_100;

/// `SOCKS5` is WinHTTP's to read, and which build reads it is pinned here: Windows 10 22H2's typed
/// answer carries it, and build 26100's drops it from the list, so Cmdr goes DIRECT around the
/// user's SOCKS proxy there (#1421). A change either way (a fix, or an older build dropping it too)
/// turns this red.
#[test]
fn a_socks5_answer_is_read_up_to_the_build_that_drops_it() {
    let five = pac_answer(r#"function FindProxyForURL(url, host) { return "SOCKS5 s.test:1081; DIRECT"; }"#);
    let expected = if windows_build() >= DROPS_VERSIONED_SOCKS {
        vec![Entry::Direct]
    } else {
        vec![socks("s.test", 1081), Entry::Direct]
    };
    assert_eq!(five, Some(expected), "build {}", windows_build());
}

/// `SOCKS4` comes through as SOCKS4 (❌ never a SOCKS5 entry it can't speak) up to build 26100,
/// which drops it from the list as it drops `SOCKS5` (#1421), pinned the same way.
#[test]
fn a_socks4_answer_is_a_socks4_entry_up_to_the_build_that_drops_it() {
    let four = pac_answer(r#"function FindProxyForURL(url, host) { return "SOCKS4 s.test:1082"; }"#);
    let expected = if windows_build() >= DROPS_VERSIONED_SOCKS {
        vec![]
    } else {
        vec![Entry::Socks4 {
            host: "s.test".into(),
            port: 1082,
        }]
    };
    assert_eq!(four, Some(expected), "build {}", windows_build());
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
