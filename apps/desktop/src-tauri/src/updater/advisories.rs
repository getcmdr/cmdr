//! The security notice an update carries, from a SIGNED advisory list: the staged bundle's own on
//! macOS (read by `platform/macos/installer.rs` once `vet_staged_bundle` trusts the bundle), the
//! release's minisign-signed asset on Windows and Linux ([`fetch_signed_list`]).
//!
//! ❗ Never from `latest.json` itself: the manifest isn't signed, so a field there would let
//! whoever can serve it show a fake "critical" notice with a phishing link, or hide a real one.
//! `DETAILS.md` § Security notices.

use semver::Version;

use crate::security_advisories::{self, Severity};

/// What the restart toast says about security: the update fixes `count` advisories, the most
/// severe of them `severity`, whose public note is `url`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SecurityNotice {
    pub severity: Severity,
    pub count: u32,
    /// The most severe advisory's link, or `None` when the allowlist refuses it.
    pub url: Option<String>,
}

/// The notice for moving from `running` to `staged`, from the staged bundle's list `json`: the
/// advisories in `running < fixedIn <= staged`, or `None` when there are none or the list doesn't
/// parse (logged at warn by the parser).
pub(super) fn notice_from(json: &str, running: &Version, staged: &Version) -> Option<SecurityNotice> {
    let advisories = security_advisories::parse(json, "staged bundle's")?;
    let fixed = security_advisories::fixed_between(&advisories, running, staged);
    let most_severe = fixed.first()?;
    Some(SecurityNotice {
        severity: most_severe.severity,
        count: u32::try_from(fixed.len()).unwrap_or(u32::MAX),
        url: security_advisories::vetted_link(most_severe),
    })
}

/// How long fetching the signed list may take, end to end. The list is a few hundred bytes, and
/// the update is ready either way.
const SIGNED_LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// The most the signed list may weigh before the fetch gives up on it (a few hundred bytes today).
const MAX_SIGNED_LIST_BYTES: usize = 256 << 10;

/// Fetches the release's signed advisory list (`latest.json`'s `securityAdvisories`) for an
/// install that can't open its update before running it (Windows, Linux), or `None` (logged at
/// warn) when there's none, it doesn't arrive, or its signature doesn't hold: an update never
/// waits on, or fails over, its advisory list.
///
/// The minisign signature over the list is what makes it trustworthy, exactly as the bundle's
/// code signature does on macOS; the manifest naming it isn't signed. A manifest can still leave
/// the entry out and hide a notice, as it could withhold the update itself, but it can't invent
/// one. `notice_from` keeps `fixedIn <= staged`, so a list from a later release can't speak for
/// fixes this update doesn't carry.
pub(super) async fn fetch_signed_list(entry: &super::manifest::PlatformEntry) -> Option<String> {
    let client = cmdr_http::client_builder()
        .connect_timeout(SIGNED_LIST_TIMEOUT)
        .timeout(SIGNED_LIST_TIMEOUT)
        .build()
        .inspect_err(|e| log::warn!("Couldn't build the client for the security advisory list: {e}"))
        .ok()?;
    let response = crate::server_request::send(crate::managed_policy::Egress::UpdateDownload, client.get(&entry.url))
        .await
        .inspect_err(|e| {
            log::warn!("Couldn't fetch the update's security advisory list, so it names no advisory: {e:?}")
        })
        .ok()?;
    let bytes = crate::server_request::read_capped(response, MAX_SIGNED_LIST_BYTES)
        .await
        .inspect_err(|e| log::warn!("Couldn't read the update's security advisory list: {e}"))
        .ok()?;
    verified_list(super::signature::verify(&bytes, &entry.signature), &bytes)
}

/// The list's text once its signature held, or `None` (warn) when it didn't.
fn verified_list(verified: Result<super::signature::VerifiedSignature, String>, bytes: &[u8]) -> Option<String> {
    if let Err(refusal) = verified {
        log::warn!("The update's security advisory list isn't signed by Cmdr, so it names no advisory: {refusal}");
        return None;
    }
    String::from_utf8(bytes.to_vec())
        .inspect_err(|e| log::warn!("The update's security advisory list isn't UTF-8: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(raw: &str) -> Version {
        Version::parse(raw).expect("test versions are semver")
    }

    const LIST: &str = r#"{"advisories":[
        {"id":"GHSA-low1-0000-0000","fixedIn":"0.51.0","severity":"low","url":"https://github.com/vdavid/cmdr/security/advisories/GHSA-low1-0000-0000"},
        {"id":"GHSA-crit-0000-0000","fixedIn":"0.52.0","severity":"critical","url":"https://github.com/vdavid/cmdr/security/advisories/GHSA-crit-0000-0000"},
        {"id":"2026-10-16-medium","fixedIn":"0.54.0","severity":"medium","url":"https://getcmdr.com/changelog"}
    ]}"#;

    /// 0.50 straight to 0.55 skips the releases that fixed them, and still hears about all three.
    #[test]
    fn a_jump_over_several_releases_counts_every_fix_and_links_the_worst() {
        assert_eq!(
            notice_from(LIST, &version("0.50.0"), &version("0.55.0")),
            Some(SecurityNotice {
                severity: Severity::Critical,
                count: 3,
                url: Some("https://github.com/vdavid/cmdr/security/advisories/GHSA-crit-0000-0000".to_string()),
            })
        );
    }

    #[test]
    fn only_fixes_newer_than_the_running_build_count() {
        assert_eq!(
            notice_from(LIST, &version("0.52.0"), &version("0.54.0")),
            Some(SecurityNotice {
                severity: Severity::Medium,
                count: 1,
                url: Some("https://getcmdr.com/changelog".to_string()),
            })
        );
    }

    #[test]
    fn an_update_with_no_fix_in_its_window_has_no_notice() {
        assert_eq!(notice_from(LIST, &version("0.54.0"), &version("0.55.0")), None);
    }

    /// The update is the security fix itself, so a broken list costs the notice, never the update.
    #[test]
    fn a_malformed_list_has_no_notice() {
        assert_eq!(notice_from("{", &version("0.50.0"), &version("0.55.0")), None);
    }

    /// A severity this build doesn't know drops that advisory alone: the others still make the
    /// notice, worst first.
    #[test]
    fn an_unknown_severity_drops_only_its_advisory() {
        let unknown = LIST.replace("\"critical\"", "\"apocalyptic\"");
        let notice = notice_from(&unknown, &version("0.50.0"), &version("0.55.0")).expect("two still count");
        assert_eq!((notice.severity, notice.count), (Severity::Medium, 2));
    }

    #[test]
    fn a_most_severe_link_the_allowlist_refuses_keeps_the_notice_without_it() {
        let list = LIST.replace(
            "https://github.com/vdavid/cmdr/security/advisories/GHSA-crit-0000-0000",
            "https://github.com.evil.example/advisory",
        );
        let notice = notice_from(&list, &version("0.50.0"), &version("0.55.0")).expect("still a notice");
        assert_eq!(notice.severity, Severity::Critical);
        assert_eq!(notice.url, None);
    }

    const SIGNED_LIST: &[u8] = include_bytes!("testdata/security-advisories.json");
    const SIGNED_LIST_SIG: &str = include_str!("testdata/security-advisories.json.sig");
    const VERSIONED_PUB: &str = include_str!("testdata/versioned-fixture.pub");

    /// The Windows and Linux path: a list whose minisign signature holds yields its notice.
    #[test]
    fn a_signed_list_names_its_notice() {
        let verified = super::super::signature::verify_with(&[VERSIONED_PUB], SIGNED_LIST, SIGNED_LIST_SIG);
        let json = verified_list(verified, SIGNED_LIST).expect("a signed list reads");
        assert_eq!(
            notice_from(&json, &version("0.52.0"), &version("0.53.0")).map(|n| (n.severity, n.count)),
            Some((Severity::High, 1))
        );
    }

    /// Hostile: a list the updater key didn't sign (a tampered or invented one) shows nothing,
    /// whatever severity it claims.
    #[test]
    fn a_list_whose_signature_fails_names_no_advisory() {
        let forged = String::from_utf8(SIGNED_LIST.to_vec())
            .unwrap()
            .replace("\"high\"", "\"critical\"");
        let verified = super::super::signature::verify_with(&[VERSIONED_PUB], forged.as_bytes(), SIGNED_LIST_SIG);
        assert_eq!(verified_list(verified, forged.as_bytes()), None);
        // And a genuine list checked against keys that didn't sign it.
        assert_eq!(
            verified_list(
                super::super::signature::verify(SIGNED_LIST, SIGNED_LIST_SIG),
                SIGNED_LIST
            ),
            None
        );
    }
}
