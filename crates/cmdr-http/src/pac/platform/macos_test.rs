//! What only CFNetwork does: inline scripts, and its reading of a PAC's SOCKS answers. The
//! cache, budget, and timeout cells every PAC runner shares are `crate::pac`'s `pac_test.rs`.

use crate::entries::{Entry, Pac, PacSource};
use crate::pac::test_server::{SCRIPT, url};
use crate::pac::{ANSWER_BUDGET, OsPac, TIMEOUT};

fn via_proxy() -> Vec<Entry> {
    vec![
        Entry::Http {
            host: "proxy.test".into(),
            port: 3128,
            credentials: None,
        },
        Entry::Direct,
    ]
}

#[test]
fn an_inline_pac_script_runs_too() {
    let evaluator = OsPac::new(ANSWER_BUDGET);
    let source = PacSource::Script(SCRIPT.into());
    assert_eq!(
        evaluator.evaluate(&source, &url("http://example.com")),
        Some(via_proxy())
    );
}

/// What CFNetwork makes of a PAC that answers `answer` for every URL.
fn pac_answer(answer: &str) -> Option<Vec<Entry>> {
    let script = format!(r#"function FindProxyForURL(url, host) {{ return "{answer}"; }}"#);
    OsPac::new(ANSWER_BUDGET).evaluate(&PacSource::Script(script), &url("https://example.com"))
}

fn socks_test(port: u16) -> Entry {
    Entry::Socks {
        host: "socks.test".into(),
        port,
        credentials: None,
    }
}

#[test]
fn a_socks_answer_is_a_socks_entry() {
    assert_eq!(
        pac_answer("SOCKS socks.test:1080; DIRECT"),
        Some(vec![socks_test(1080), Entry::Direct])
    );
    assert_eq!(pac_answer("socks socks.test:1081"), Some(vec![socks_test(1081)]));
}

#[test]
fn cfnetwork_drops_the_versioned_socks_keywords() {
    // CFNetwork's PAC parser knows `SOCKS` alone; `SOCKS5` and `SOCKS4` (Chrome and Firefox
    // extensions) vanish from the list, so they're as invisible to Cmdr as to Safari (verified on
    // macOS 27.0, 2026-10-08). A regression anchor: if this starts failing, macOS learned them.
    assert_eq!(pac_answer("SOCKS5 socks.test:1080; DIRECT"), Some(vec![Entry::Direct]));
    assert_eq!(pac_answer("SOCKS4 socks.test:1080; DIRECT"), Some(vec![Entry::Direct]));
    assert_eq!(pac_answer("SOCKS5 socks.test:1080"), Some(vec![]));
}

#[test]
fn a_broken_script_is_a_failure() {
    let evaluator = OsPac::new(TIMEOUT);
    let answer = evaluator.evaluate(
        &PacSource::Script("this is not javascript (".into()),
        &url("https://example.com"),
    );
    assert_eq!(answer, None);
}

/// Nothing on a Mac produces a WPAD source (macOS hands discovery's answer over as a PAC URL),
/// so its runner refuses one rather than guessing.
#[test]
fn auto_detect_has_no_runner_on_macos() {
    assert_eq!(
        super::execute(&PacSource::AutoDetect, "https://example.com", TIMEOUT),
        None
    );
}
