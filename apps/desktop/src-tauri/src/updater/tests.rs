//! Tests for the update commands: the skip gate, the managed policy at each step, and typed request failures.

use super::*;
use std::collections::HashSet;

/// Asks the gate about a process running from a macOS bundle (`in_app_bundle`) or a dev binary,
/// whose environment holds exactly `vars` and nothing else.
fn skip(in_app_bundle: bool, vars: &[&str]) -> Option<SkipReason> {
    let install = if in_app_bundle {
        Ok(InstallKind::MacAppBundle)
    } else {
        Err(NotUpdatable::NotAnAppBundle)
    };
    skip_with(install, vars)
}

fn skip_with(install: Result<InstallKind, NotUpdatable>, vars: &[&str]) -> Option<SkipReason> {
    let set: HashSet<&str> = vars.iter().copied().collect();
    skip_reason_for(install, &|name| set.contains(name))
}

const NOT_A_BUNDLE: SkipReason = SkipReason::NotUpdatable(NotUpdatable::NotAnAppBundle);

#[test]
fn a_bundled_release_with_clean_env_may_check() {
    assert_eq!(skip(true, &[]), None);
}

#[test]
fn an_unbundled_build_never_checks() {
    assert_eq!(skip(false, &[]), Some(NOT_A_BUNDLE));
}

/// Windows and Linux share the gate: an installed NSIS build or AppImage checks, a dev binary or
/// a package-manager install never does, and every tooling env var still suppresses.
#[test]
fn windows_and_linux_installs_pass_the_same_gate() {
    for kind in [InstallKind::WindowsNsis, InstallKind::LinuxAppImage] {
        assert_eq!(skip_with(Ok(kind), &[]), None, "{kind:?}");
        assert_eq!(
            skip_with(Ok(kind), &["CMDR_E2E_MODE"]),
            Some(SkipReason::NonProdEnv("CMDR_E2E_MODE"))
        );
    }
    for reason in [NotUpdatable::NotAnInstalledBuild, NotUpdatable::PackageManagerInstall] {
        assert_eq!(skip_with(Err(reason), &[]), Some(SkipReason::NotUpdatable(reason)));
    }
}

/// The URL every macOS install has always sent; the dashboard's arch split depends on it.
#[test]
fn the_check_url_keeps_its_macos_shape_and_names_other_oses() {
    assert_eq!(
        update_check_url("0.51.0", InstallKind::MacAppBundle, Arch::Aarch64),
        "https://api.getcmdr.com/update-check/0.51.0?arch=aarch64"
    );
    assert_eq!(
        update_check_url("0.51.0", InstallKind::WindowsNsis, Arch::X86_64),
        "https://api.getcmdr.com/update-check/0.51.0?arch=windows-x86_64"
    );
}

/// Every non-prod signal suppresses on its own, even from a properly bundled app. A bundled
/// harness run is exactly the case the old `CI`-only gate let through.
#[test]
fn each_non_prod_env_var_suppresses_a_bundled_app() {
    for name in crate::prod_instance::NON_PROD_ENV_VARS {
        assert_eq!(
            skip(true, &[name]),
            Some(SkipReason::NonProdEnv(name)),
            "{name} alone must keep a bundled app off the update-check endpoint"
        );
    }
}

/// The gate and the analytics gate must agree about what a real install is, or the dashboard's
/// `update_checks` ceiling and its heartbeat floor start counting different populations.
#[test]
fn every_tooling_launcher_is_suppressed_even_when_bundled() {
    // `scripts/check/checks/e2e-playwright-app.go`.
    let e2e_checker = ["CMDR_INSTANCE_ID", "CMDR_DATA_DIR", "CMDR_E2E_MODE", "CMDR_MOCK_FDA"];
    // `apps/desktop/scripts/i18n-capture.ts`.
    let i18n_capture = ["CMDR_E2E_MODE", "CMDR_DATA_DIR", "CMDR_MOCK_FDA"];
    // `apps/desktop/scripts/marketing-launch.ts` (shots and clips) deliberately leaves `CMDR_E2E_MODE` unset.
    let marketing_shots = ["CMDR_DATA_DIR"];
    // `apps/desktop/scripts/tauri-wrapper.ts` (dev and per-worktree dev).
    let dev_wrapper = ["CMDR_INSTANCE_ID", "CMDR_DATA_DIR"];

    for (label, vars) in [
        ("e2e checker", &e2e_checker[..]),
        ("i18n capture", &i18n_capture[..]),
        ("marketing shots", &marketing_shots[..]),
        ("dev wrapper", &dev_wrapper[..]),
    ] {
        assert!(
            skip(true, vars).is_some(),
            "{label} must not reach the update-check endpoint"
        );
    }
}

/// The bundle condition wins, so the log names the thing that makes an update impossible
/// rather than one that merely makes it unwanted.
#[test]
fn the_bundle_condition_is_reported_first() {
    assert_eq!(skip(false, &["CMDR_E2E_MODE"]), Some(NOT_A_BUNDLE));
}

/// The log has to name the condition, or the next pollution incident is undiagnosable.
#[test]
fn reasons_name_the_condition() {
    assert_eq!(NOT_A_BUNDLE.to_string(), "not running from a .app bundle");
    assert_eq!(
        SkipReason::NonProdEnv("CMDR_DATA_DIR").to_string(),
        "CMDR_DATA_DIR is set"
    );
}

use crate::managed_policy::testing::{
    DISABLE_AUTOMATIC_UPDATE_CHECKS, DISABLE_UPDATES, MAX_UPDATE_VERSION, forcing, from_values, override_for_test,
};
use std::cell::Cell;

const RUNNING: &str = "0.51.0";
const TEST_KEY: &str = "darwin-aarch64";

fn test_keys() -> Vec<String> {
    vec![TEST_KEY.to_string()]
}

fn manifest_offering(version: &str) -> manifest::UpdateManifest {
    manifest::UpdateManifest {
        version: version.to_string(),
        platforms: [(
            TEST_KEY.to_string(),
            manifest::PlatformEntry {
                url: "https://example.invalid/Cmdr.app.tar.gz".to_string(),
                signature: "sig".to_string(),
            },
        )]
        .into(),
        security_advisories: None,
    }
}

fn ceiling(text: &str) -> crate::managed_policy::ManagedPolicy {
    from_values(&[(MAX_UPDATE_VERSION, plist::Value::String(text.to_string()))])
}

/// Runs one production-install check against a server offering `offered`, and counts how many
/// times the check reached for the network.
async fn check(trigger: UpdateCheckTrigger, state: &UpdateState, offered: &str) -> (UpdateCheckOutcome, usize) {
    let fetches = Cell::new(0);
    let outcome = run_check(trigger, state, None, RUNNING, &test_keys(), || async {
        fetches.set(fetches.get() + 1);
        Ok(manifest_offering(offered))
    })
    .await
    .expect("an injected manifest always lands");
    (outcome, fetches.get())
}

const EVERY_TRIGGER: [UpdateCheckTrigger; 5] = [
    UpdateCheckTrigger::Startup,
    UpdateCheckTrigger::Poll,
    UpdateCheckTrigger::AutoCheckOn,
    UpdateCheckTrigger::Command,
    UpdateCheckTrigger::Settings,
];

#[tokio::test]
async fn no_policy_offers_a_newer_release_and_remembers_it() {
    let state = UpdateState::new();
    let (outcome, fetches) = check(UpdateCheckTrigger::Poll, &state, "0.53.0").await;
    assert_eq!(
        outcome,
        UpdateCheckOutcome::Available {
            version: "0.53.0".into()
        }
    );
    assert_eq!(fetches, 1);
    let offer = offer_to_download(&state).await.expect("the check offered it");
    assert_eq!(offer.version, semver::Version::new(0, 53, 0));
}

#[tokio::test]
async fn the_same_version_is_up_to_date_and_offers_nothing() {
    let state = UpdateState::new();
    let (outcome, _) = check(UpdateCheckTrigger::Poll, &state, RUNNING).await;
    assert_eq!(outcome, UpdateCheckOutcome::UpToDate);
    assert_eq!(
        offer_to_download(&state).await.expect_err("nothing offered"),
        UpdateDownloadError::NothingOffered
    );
}

/// `DisableUpdates` means no request to `api.getcmdr.com/update-check` at all, from any entry point.
#[tokio::test]
async fn updates_off_never_reaches_the_network() {
    let _policy = override_for_test(forcing(&[DISABLE_UPDATES]));
    for trigger in EVERY_TRIGGER {
        let state = UpdateState::new();
        let (outcome, fetches) = check(trigger, &state, "0.53.0").await;
        assert_eq!(outcome, UpdateCheckOutcome::UpdatesDisabledByPolicy, "{trigger:?}");
        assert_eq!(fetches, 0, "{trigger:?} must not fetch");
    }
}

#[tokio::test]
async fn automatic_checks_off_refuses_only_the_background_triggers() {
    let _policy = override_for_test(forcing(&[DISABLE_AUTOMATIC_UPDATE_CHECKS]));
    for trigger in EVERY_TRIGGER {
        let state = UpdateState::new();
        let (outcome, fetches) = check(trigger, &state, "0.53.0").await;
        if trigger.is_automatic() {
            assert_eq!(
                outcome,
                UpdateCheckOutcome::AutomaticChecksDisabledByPolicy,
                "{trigger:?}"
            );
            assert_eq!(fetches, 0, "{trigger:?} must not fetch");
        } else {
            assert!(
                matches!(outcome, UpdateCheckOutcome::Available { .. }),
                "a person asking still checks: {trigger:?} got {outcome:?}"
            );
        }
    }
    assert!(!UpdateCheckTrigger::Command.is_automatic());
    assert!(!UpdateCheckTrigger::Settings.is_automatic());
}

#[tokio::test]
async fn a_release_past_the_ceiling_is_held_and_not_offered() {
    let _policy = override_for_test(ceiling("0.52"));
    let state = UpdateState::new();
    let (outcome, fetches) = check(UpdateCheckTrigger::Command, &state, "0.53.0").await;
    assert_eq!(
        outcome,
        UpdateCheckOutcome::HeldByPolicy {
            available: "0.53.0".into(),
            ceiling: "0.52".into()
        }
    );
    assert_eq!(
        fetches, 1,
        "a held check still asks: it's how the person learns a release exists"
    );
    assert_eq!(
        offer_to_download(&state)
            .await
            .expect_err("a held release is never offered"),
        UpdateDownloadError::NothingOffered
    );
}

#[tokio::test]
async fn a_prerelease_of_the_next_minor_is_held_too() {
    let _policy = override_for_test(ceiling("0.52"));
    let (outcome, _) = check(UpdateCheckTrigger::Command, &UpdateState::new(), "0.53.0-rc.1").await;
    assert!(
        matches!(outcome, UpdateCheckOutcome::HeldByPolicy { .. }),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_release_under_the_ceiling_is_offered() {
    let _policy = override_for_test(ceiling("0.52"));
    let (outcome, _) = check(UpdateCheckTrigger::Poll, &UpdateState::new(), "0.52.4").await;
    assert_eq!(
        outcome,
        UpdateCheckOutcome::Available {
            version: "0.52.4".into()
        }
    );
}

/// A later check that holds forgets the earlier offer, so the download can't fetch a release the
/// newest policy refuses.
#[tokio::test]
async fn every_check_replaces_the_last_offer() {
    let state = UpdateState::new();
    check(UpdateCheckTrigger::Poll, &state, "0.53.0").await;
    let _policy = override_for_test(forcing(&[DISABLE_UPDATES]));
    check(UpdateCheckTrigger::Command, &state, "0.53.0").await;
    assert_eq!(state.slots().offered.as_ref().map(|o| o.version.to_string()), None);
}

/// Outside a production install the check never fetches, but the policy still answers first, so
/// a dev run with `CMDR_MANAGED_PREFS_FILE` shows what a managed Mac would.
#[tokio::test]
async fn a_dev_build_answers_the_policy_and_otherwise_never_fetches() {
    let state = UpdateState::new();
    let fetched = Cell::new(false);
    let fetch = || async {
        fetched.set(true);
        Ok(manifest_offering("0.53.0"))
    };
    let outcome = run_check(
        UpdateCheckTrigger::Poll,
        &state,
        Some(NOT_A_BUNDLE),
        RUNNING,
        &[],
        fetch,
    )
    .await
    .expect("no fetch, no failure");
    assert_eq!(outcome, UpdateCheckOutcome::UpToDate);

    let _policy = override_for_test(forcing(&[DISABLE_UPDATES]));
    let outcome = run_check(
        UpdateCheckTrigger::Poll,
        &state,
        Some(NOT_A_BUNDLE),
        RUNNING,
        &[],
        fetch,
    )
    .await
    .expect("no fetch, no failure");
    assert_eq!(outcome, UpdateCheckOutcome::UpdatesDisabledByPolicy);
    assert!(!fetched.get());
}

/// A profile that arrives between the check and the download stops the download before a byte.
#[tokio::test]
async fn a_policy_that_arrives_after_the_check_blocks_the_download() {
    let state = UpdateState::new();
    check(UpdateCheckTrigger::Poll, &state, "0.53.0").await;
    for policy in [ceiling("0.52"), forcing(&[DISABLE_UPDATES])] {
        let _policy = override_for_test(policy);
        assert_eq!(
            offer_to_download(&state).await.expect_err("refused"),
            UpdateDownloadError::BlockedByPolicy
        );
    }
}

fn test_offer(version: &str, url: &str) -> UpdateInfo {
    UpdateInfo {
        version: semver::Version::parse(version).expect("a valid test version"),
        platform_key: TEST_KEY.to_string(),
        url: url.to_string(),
        signature: "not-a-signature".to_string(),
        security_advisories: None,
    }
}

fn staged(state: &UpdateState, version: &str) {
    state.slots().downloaded = Some(DownloadedUpdate {
        offer: test_offer(version, "https://example.invalid/Cmdr.app.tar.gz"),
        file: PathBuf::from("/nonexistent/Cmdr.app.tar.gz"),
        pubkey: "key",
    });
}

#[tokio::test]
async fn install_with_nothing_downloaded_refuses() {
    let err = staged_to_install(&UpdateState::new())
        .await
        .expect_err("nothing staged");
    assert_eq!(err, UpdateInstallError::NothingStaged);
}

/// A download staged before the profile arrived must not install.
#[tokio::test]
async fn a_policy_that_arrives_after_the_download_blocks_the_install() {
    for policy in [ceiling("0.52"), forcing(&[DISABLE_UPDATES])] {
        let state = UpdateState::new();
        staged(&state, "0.53.0");
        let _policy = override_for_test(policy);
        let capture = crate::security_log::testing::capture_events();
        let err = staged_to_install(&state).await.expect_err("refused");
        assert_eq!(err, UpdateInstallError::BlockedByPolicy);
        assert!(
            matches!(
                capture.events().as_slice(),
                [crate::security_log::SecurityEvent::UpdateRefusedByPolicy { version, .. }] if version == "0.53.0"
            ),
            "the refusal reaches the security log: {:?}",
            capture.events()
        );
    }
}

#[tokio::test]
async fn a_staged_version_the_policy_allows_installs() {
    let _policy = override_for_test(ceiling("0.53"));
    let state = UpdateState::new();
    staged(&state, "0.53.0");
    let (update, _) = staged_to_install(&state).await.expect("allowed");
    assert_eq!(update.offer.version, semver::Version::new(0, 53, 0));
}

use crate::server_request::ServerRequestError;
use serde_json::json;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A mock server answering every GET with `response`, and the manifest URL on it. Keep the server
/// bound for the test's length: dropping it stops the mock.
async fn manifest_at(response: ResponseTemplate) -> (MockServer, String) {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(response).mount(&server).await;
    let url = format!("{}/latest.json", server.uri());
    (server, url)
}

#[tokio::test]
async fn a_manifest_the_server_serves_parses() {
    let (_server, url) = manifest_at(ResponseTemplate::new(200).set_body_json(json!({
        "version": "0.45.1",
        "platforms": { "darwin-aarch64": { "url": "https://example.invalid/Cmdr.tar.gz", "signature": "sig" } }
    })))
    .await;
    let manifest = fetch_manifest(&url).await.expect("a well-formed manifest parses");
    assert_eq!(manifest.version, "0.45.1");
}

/// The shape behind the old "Couldn't parse update manifest" lines: a 2xx that isn't a manifest
/// means Cmdr's server and this build disagree, which the frontend logs at error. It must never
/// read as a network blip.
#[tokio::test]
async fn a_2xx_that_isnt_a_manifest_is_a_bad_response() {
    let (_server, url) = manifest_at(ResponseTemplate::new(200).set_body_json(json!({ "version": "0.45.1" }))).await;
    let err = fetch_manifest(&url)
        .await
        .expect_err("a manifest without platforms doesn't parse");
    assert!(
        matches!(err, ServerRequestError::BadResponse { .. }),
        "expected BadResponse, got {err:?}"
    );
}

/// A host having a bad moment is a request failure the frontend logs at warn. It used to read the
/// maintenance page as tarball bytes and report a signature mismatch.
#[tokio::test]
async fn a_tarball_host_answering_503_is_a_request_failure_not_a_bad_signature() {
    let (_server, url) = manifest_at(ResponseTemplate::new(503).set_body_string("<html>maintenance</html>")).await;
    let err = fetch_verified(&url, "sig", None)
        .await
        .expect_err("a 503 brings no tarball");
    assert!(
        matches!(
            err,
            UpdateDownloadError::Request {
                failure: ServerRequestError::Refused { status: 503, .. }
            }
        ),
        "expected a 503 request failure, got {err:?}"
    );
}

/// Bytes that don't match the signature stay their own kind, which the frontend logs at error.
#[tokio::test]
async fn a_tarball_that_fails_its_signature_is_a_signature_mismatch() {
    let (_server, url) = manifest_at(ResponseTemplate::new(200).set_body_bytes(b"not the real tarball".to_vec())).await;
    let err = fetch_verified(&url, "not-a-signature", None)
        .await
        .expect_err("garbage doesn't verify");
    assert!(
        matches!(err, UpdateDownloadError::SignatureMismatch { .. }),
        "expected SignatureMismatch, got {err:?}"
    );
}

#[tokio::test]
async fn an_unreachable_tarball_host_is_a_request_failure() {
    // Port 9 (discard) on loopback refuses the connection.
    let err = fetch_verified("http://127.0.0.1:9/Cmdr.app.tar.gz", "sig", None)
        .await
        .expect_err("nothing listens there");
    assert!(
        matches!(
            err,
            UpdateDownloadError::Request {
                failure: ServerRequestError::Unreachable { .. }
            }
        ),
        "expected an unreachable request failure, got {err:?}"
    );
}

/// A maintenance page on a 5xx stays a refusal with its status, never "the manifest is malformed".
#[tokio::test]
async fn a_5xx_maintenance_page_is_refused_not_malformed() {
    let (_server, url) = manifest_at(ResponseTemplate::new(503).set_body_string("<html>maintenance</html>")).await;
    let err = fetch_manifest(&url).await.expect_err("a 503 is a refusal");
    assert!(
        matches!(err, ServerRequestError::Refused { status: 503, .. }),
        "expected a 503 Refused, got {err:?}"
    );
}

fn offer(state: &UpdateState, version: &str, url: &str) {
    state.slots().offered = Some(test_offer(version, url));
}

/// An offered version that didn't carry Cmdr's signature is fetched once a session: the
/// background loop offering it again doesn't download it again (or tell the person twice), and a
/// newer release still gets through, so a real fix can arrive (#457, David's call 2026-10-16).
#[tokio::test]
async fn a_version_that_failed_its_signature_is_not_fetched_again_this_session() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not the real tarball".to_vec()))
        .expect(1)
        .mount(&server)
        .await;
    let url = format!("{}/Cmdr.app.tar.gz", server.uri());
    let state = UpdateState::new();
    let capture = crate::security_log::testing::capture_events();

    offer(&state, "0.53.0", &url);
    let first = download_verified(&state, InstallKind::MacAppBundle)
        .await
        .expect_err("garbage doesn't verify");
    assert!(
        matches!(first, UpdateDownloadError::SignatureMismatch { .. }),
        "{first:?}"
    );

    offer(&state, "0.53.0", &url);
    assert_eq!(
        download_verified(&state, InstallKind::MacAppBundle)
            .await
            .expect_err("refused before fetching"),
        UpdateDownloadError::NotGenuineEarlier
    );
    assert_eq!(
        capture.events(),
        vec![crate::security_log::SecurityEvent::UpdateSignatureInvalid {
            version: "0.53.0".into(),
            check: crate::security_log::SignatureCheck::Minisign,
        }],
        "the record and the notice hear it once"
    );

    offer(&state, "0.53.1", &url);
    assert_eq!(
        offer_to_download(&state)
            .await
            .expect("a newer release is still offered")
            .version,
        semver::Version::new(0, 53, 1)
    );
}

/// A bundle refused for its code signature at install is the same: that version isn't fetched
/// again this session.
#[tokio::test]
async fn a_version_refused_for_its_code_signature_is_not_fetched_again_this_session() {
    let state = UpdateState::new();
    let version = semver::Version::new(0, 53, 0);
    note_install_refusal(
        &state,
        &version,
        &UpdateInstallError::UntrustedCodeSignature { detail: "x".into() },
    );
    note_install_refusal(
        &state,
        &semver::Version::new(0, 53, 1),
        &UpdateInstallError::Failed { detail: "x".into() },
    );
    offer(&state, "0.53.0", "https://example.invalid/Cmdr.app.tar.gz");
    assert_eq!(
        offer_to_download(&state).await.expect_err("refused"),
        UpdateDownloadError::NotGenuineEarlier
    );
    offer(&state, "0.53.1", "https://example.invalid/Cmdr.app.tar.gz");
    assert!(
        offer_to_download(&state).await.is_ok(),
        "a broken install isn't a fake one"
    );
}

fn verified_for(signed_version: Option<&str>) -> signature::VerifiedSignature {
    signature::VerifiedSignature {
        pubkey: "key",
        signed_version: signed_version.map(str::to_string),
    }
}

/// Windows and Linux: the signature has to name the offered version, since nothing else signed
/// does. Hostile: an unsigned manifest offering "0.54.0" in front of a genuine 0.52.0 installer is
/// a rollback, and a signature with no version can't vouch for anything.
#[test]
fn a_signature_for_another_version_or_none_is_refused_off_macos() {
    let offered = semver::Version::new(0, 54, 0);
    assert_eq!(bind_signed_version(&verified_for(Some("0.54.0")), &offered), Ok(()));
    assert_eq!(bind_signed_version(&verified_for(Some("v0.54.0")), &offered), Ok(()));
    for signed in [Some("0.52.0"), Some("0.54.0-rc.1"), Some("not-a-version"), None] {
        assert!(
            bind_signed_version(&verified_for(signed), &offered).is_err(),
            "{signed:?} must not vouch for 0.54.0"
        );
    }
}

/// Hostile: a tampered Windows installer is refused like a tampered macOS tarball, and its version
/// isn't fetched again this session.
#[tokio::test]
async fn a_tampered_installer_is_refused_and_not_fetched_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not the real installer".to_vec()))
        .expect(1)
        .mount(&server)
        .await;
    let url = format!("{}/Cmdr_0.53.0_x64-setup.exe", server.uri());
    let state = UpdateState::new();
    offer(&state, "0.53.0", &url);
    let err = download_verified(&state, InstallKind::WindowsNsis)
        .await
        .expect_err("garbage doesn't verify");
    assert!(matches!(err, UpdateDownloadError::SignatureMismatch { .. }), "{err:?}");
    offer(&state, "0.53.0", &url);
    assert_eq!(
        offer_to_download(&state).await.expect_err("remembered"),
        UpdateDownloadError::NotGenuineEarlier
    );
}
