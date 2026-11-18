//! What the updater notes in the security log (`crate::security_log`): an install, a policy
//! refusal, and a signature that didn't hold. Kept out of `mod.rs` so each command carries a
//! one-line hook. ❌ Never a `detail` string: only the version and the typed reason.

use super::{UpdateDownloadError, UpdateInstallError};
use crate::managed_policy::UpdateRefusal;
use crate::security_log::{self, PolicyRefusal, SecurityEvent, SignatureCheck};

/// A download that didn't verify against the minisign signature the manifest named.
pub(super) fn on_download_error(version: &semver::Version, error: &UpdateDownloadError) {
    if matches!(error, UpdateDownloadError::SignatureMismatch { .. }) {
        security_log::record(SecurityEvent::UpdateSignatureInvalid {
            version: version.to_string(),
            check: SignatureCheck::Minisign,
        });
    }
}

/// The organization's policy refused installing `version`.
pub(super) fn on_policy_refusal(version: &semver::Version, refusal: UpdateRefusal) {
    security_log::record(SecurityEvent::UpdateRefusedByPolicy {
        version: version.to_string(),
        reason: match refusal {
            UpdateRefusal::Disabled => PolicyRefusal::UpdatesDisabled,
            UpdateRefusal::AboveCeiling(_) => PolicyRefusal::AboveCeiling,
        },
    });
}

/// Windows and Linux: the staged installer goes to the plugin as Cmdr exits. Recorded as the
/// install, since it's the last moment the log can write (`handoff::note_handoff`).
pub(super) fn on_handoff(version: &semver::Version) {
    security_log::record(SecurityEvent::UpdateInstalled {
        from_version: env!("CARGO_PKG_VERSION").to_string(),
        to_version: version.to_string(),
    });
}

/// The install's outcome: `offered` is the version the check offered, `result` the version the
/// bundle itself named once installed.
pub(super) fn on_install_result(offered: &semver::Version, result: Result<&semver::Version, &UpdateInstallError>) {
    match result {
        Ok(installed) => security_log::record(SecurityEvent::UpdateInstalled {
            from_version: env!("CARGO_PKG_VERSION").to_string(),
            to_version: installed.to_string(),
        }),
        Err(UpdateInstallError::UntrustedCodeSignature { .. }) => {
            security_log::record(SecurityEvent::UpdateSignatureInvalid {
                version: offered.to_string(),
                check: SignatureCheck::CodeSignature,
            })
        }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_policy::UpdateCeiling;
    use crate::security_log::testing;

    fn v(text: &str) -> semver::Version {
        semver::Version::parse(text).expect("a valid test version")
    }

    #[test]
    fn a_minisign_mismatch_is_noted_and_other_download_failures_are_not() {
        let capture = testing::capture_events();
        on_download_error(&v("0.53.0"), &UpdateDownloadError::NothingOffered);
        on_download_error(
            &v("0.53.0"),
            &UpdateDownloadError::SignatureMismatch {
                detail: "a secret-free detail that must not reach the log".into(),
            },
        );
        assert_eq!(
            capture.events(),
            vec![SecurityEvent::UpdateSignatureInvalid {
                version: "0.53.0".into(),
                check: SignatureCheck::Minisign,
            }]
        );
    }

    #[test]
    fn an_install_notes_the_version_the_bundle_named_and_a_foreign_bundle_is_a_signature_event() {
        let capture = testing::capture_events();
        on_install_result(&v("0.53.0"), Ok(&v("0.53.1")));
        on_install_result(
            &v("0.53.0"),
            Err(&UpdateInstallError::UntrustedCodeSignature { detail: "x".into() }),
        );
        on_install_result(&v("0.53.0"), Err(&UpdateInstallError::Failed { detail: "x".into() }));
        assert_eq!(
            capture.events(),
            vec![
                SecurityEvent::UpdateInstalled {
                    from_version: env!("CARGO_PKG_VERSION").into(),
                    to_version: "0.53.1".into(),
                },
                SecurityEvent::UpdateSignatureInvalid {
                    version: "0.53.0".into(),
                    check: SignatureCheck::CodeSignature,
                },
            ]
        );
    }

    #[test]
    fn a_policy_refusal_names_its_typed_reason() {
        let capture = testing::capture_events();
        on_policy_refusal(&v("0.53.0"), UpdateRefusal::Disabled);
        on_policy_refusal(&v("0.53.0"), UpdateRefusal::AboveCeiling(UpdateCeiling::Minor(0, 52)));
        assert_eq!(
            capture.events(),
            vec![
                SecurityEvent::UpdateRefusedByPolicy {
                    version: "0.53.0".into(),
                    reason: PolicyRefusal::UpdatesDisabled,
                },
                SecurityEvent::UpdateRefusedByPolicy {
                    version: "0.53.0".into(),
                    reason: PolicyRefusal::AboveCeiling,
                },
            ]
        );
    }
}
