//! The Windows settings' rules, on every OS.

use super::*;

fn url(s: &str) -> Url {
    Url::parse(s).expect("a valid test URL")
}

fn http(host: &str, port: u16) -> Entry {
    Entry::Http {
        host: host.into(),
        port,
        credentials: None,
    }
}

fn socks(host: &str, port: u16) -> Entry {
    Entry::Socks {
        host: host.into(),
        port,
        credentials: None,
    }
}

fn manual(proxy: &str, bypass: Option<&str>) -> IeSettings {
    IeSettings {
        proxy: Some(proxy.into()),
        bypass: bypass.map(Into::into),
        ..IeSettings::default()
    }
}

#[test]
fn nothing_set_is_an_empty_list_which_walks_as_direct() {
    assert_eq!(entries_for(&IeSettings::default(), &url("https://example.com")), []);
}

#[test]
fn one_proxy_for_every_scheme() {
    let settings = manual("proxy.test:3128", None);
    assert_eq!(
        entries_for(&settings, &url("https://example.com")),
        [http("proxy.test", 3128)]
    );
    assert_eq!(
        entries_for(&settings, &url("http://example.com")),
        [http("proxy.test", 3128)]
    );
}

#[test]
fn a_proxy_without_a_port_takes_the_kinds_default() {
    assert_eq!(manual_proxy("proxy.test", "https"), Some(http("proxy.test", 80)));
    assert_eq!(
        manual_proxy("socks=socks.test", "https"),
        Some(socks("socks.test", 1080))
    );
}

/// "Use a different proxy for each protocol": each scheme takes its own, an unset one falls to
/// the SOCKS proxy, and with neither it goes direct.
#[test]
fn per_scheme_proxies_pick_the_urls_own_then_socks() {
    let both = "http=web.test:8080;https=secure.test:8443";
    assert_eq!(manual_proxy(both, "http"), Some(http("web.test", 8080)));
    assert_eq!(manual_proxy(both, "https"), Some(http("secure.test", 8443)));
    assert_eq!(manual_proxy(both, "wss"), Some(http("secure.test", 8443)));

    let http_and_socks = "http=web.test:8080;socks=socks.test:1081";
    assert_eq!(manual_proxy(http_and_socks, "https"), Some(socks("socks.test", 1081)));

    assert_eq!(manual_proxy("http=web.test:8080", "https"), None);
    assert_eq!(
        entries_for(&manual("http=web.test:8080", None), &url("https://example.com")),
        [Entry::Direct]
    );
}

#[test]
fn a_scheme_prefix_is_read_past_and_socks_one_makes_a_socks_proxy() {
    assert_eq!(
        manual_proxy("http://proxy.test:3128/", "https"),
        Some(http("proxy.test", 3128))
    );
    assert_eq!(
        manual_proxy("socks5://socks.test:1080", "https"),
        Some(socks("socks.test", 1080))
    );
}

#[test]
fn an_ipv6_proxy_keeps_its_host_whole() {
    assert_eq!(manual_proxy("[fd00::1]:3128", "https"), Some(http("fd00::1", 3128)));
    assert_eq!(manual_proxy("fd00::1", "https"), Some(http("fd00::1", 80)));
}

#[test]
fn the_bypass_list_sends_matching_hosts_direct() {
    let settings = manual("proxy.test:3128", Some("*.corp.test;getcmdr.com; 10.* ;<local>"));
    let walk = |u: &str| entries_for(&settings, &url(u));
    assert_eq!(walk("https://intranet.corp.test"), [Entry::Direct]);
    assert_eq!(walk("https://GETCMDR.com"), [Entry::Direct]);
    assert_eq!(walk("http://10.1.2.3"), [Entry::Direct]);
    assert_eq!(walk("http://printer"), [Entry::Direct], "<local> covers a dotless name");
    assert_eq!(
        walk("https://corp.test"),
        [http("proxy.test", 3128)],
        "*. needs a subdomain"
    );
    assert_eq!(walk("https://api.getcmdr.com"), [http("proxy.test", 3128)]);
    assert_eq!(walk("http://110.1.2.3"), [http("proxy.test", 3128)]);
}

#[test]
fn a_bypass_entry_with_a_scheme_or_port_matches_only_those() {
    let list = "https://secure.test;web.test:8080";
    assert!(bypasses(list, &url("https://secure.test")));
    assert!(!bypasses(list, &url("http://secure.test")));
    assert!(bypasses(list, &url("http://web.test:8080")));
    assert!(!bypasses(list, &url("http://web.test")));
}

#[test]
fn local_never_covers_an_ip_literal() {
    assert!(!bypasses("<local>", &url("http://[fd00::1]")));
    assert!(!bypasses("<-loopback>", &url("http://example.com")));
}

#[test]
fn globs_match_the_whole_host() {
    assert!(glob("*", "anything.test"));
    assert!(glob("a*b*c", "axxbyyc"));
    assert!(!glob("a*b*b", "ab"));
    assert!(!glob("corp.test", "notcorp.test"));
    assert!(glob("*corp.test", "notcorp.test"));
}

/// WPAD first, then the setup script, then the manual proxy: each a fallback for the one before.
#[test]
fn auto_detect_then_the_script_then_the_manual_proxy() {
    let settings = IeSettings {
        auto_detect: true,
        auto_config_url: Some("http://wpad.corp.test/proxy.pac".into()),
        proxy: Some("proxy.test:3128".into()),
        bypass: None,
    };
    assert_eq!(
        entries_for(&settings, &url("https://example.com")),
        [
            Entry::AutoDetect,
            Entry::AutoConfigUrl("http://wpad.corp.test/proxy.pac".into()),
            http("proxy.test", 3128),
        ]
    );
}

#[test]
fn blank_settings_count_as_unset() {
    let settings = IeSettings {
        auto_config_url: Some("  ".into()),
        proxy: Some(String::new()),
        ..IeSettings::default()
    };
    assert_eq!(entries_for(&settings, &url("https://example.com")), []);
}

#[test]
fn a_pac_answer_lists_each_proxy_then_direct() {
    assert_eq!(
        proxy_list_entries("proxy.test:3128; backup.test:8080 DIRECT"),
        [http("proxy.test", 3128), http("backup.test", 8080), Entry::Direct]
    );
    assert_eq!(proxy_list_entries(""), []);
}
