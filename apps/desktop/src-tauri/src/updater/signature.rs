//! Minisign signature verification for update tarballs.
//!
//! Both the public keys and per-release signatures are double-encoded:
//! base64(minisign-text-format). Decode the outer base64 first, then parse
//! with the minisign-verify crate. This matches Tauri's internal behavior.

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use minisign_verify::{PublicKey, Signature};

/// The main updater key, base64-encoded: the one every release is signed with. A copy of
/// `plugins.updater.pubkey` in `tauri.conf.json` (the bundler needs it there), pinned equal by a test.
const MAIN_PUBKEY_BASE64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEEyNjAxRjM2QkIxNjhDMEEK\
     UldRS2pCYTdOaDlnb2lwTE5wZUNVZE5FTTBScXRrTWJPb1EvN3J2Q0FVcC9JNzJkSmphUTl3V3MK";

/// The offline backup updater key, base64-encoded. Its private half never goes online; it signs
/// exactly one release, the one that rotates a leaked main key out. `updater-backup.pub` is its one
/// home, read here and by `scripts/release-finish` (`DETAILS.md` § Two updater keys).
const BACKUP_PUBKEY_BASE64: &str = include_str!("../../updater-backup.pub");

/// Every key an update may be signed with.
const TRUSTED_PUBKEYS: [&str; 2] = [MAIN_PUBKEY_BASE64, BACKUP_PUBKEY_BASE64];

/// What a signature that held proves beyond "these bytes are a release": which key signed them,
/// and the version the signer bound them to, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSignature {
    /// The trusted key that verified it, base64-encoded as `TRUSTED_PUBKEYS` holds it (trimmed).
    /// The installer hand-off off macOS has the plugin re-verify with this one key.
    pub pubkey: &'static str,
    /// The `version:` field of the trusted comment, which minisign's global signature covers.
    /// `tauri signer sign --app-version` writes it; `None` for a signature made without one.
    pub signed_version: Option<String>,
}

/// Verifies that `data` was signed with one of the trusted keys.
///
/// `signature_base64` is the base64-encoded minisign signature string from `latest.json`.
pub fn verify(data: &[u8], signature_base64: &str) -> Result<VerifiedSignature, String> {
    verify_with(&TRUSTED_PUBKEYS, data, signature_base64)
}

/// The check against an explicit key list, so tests can sign with a throwaway key.
pub(super) fn verify_with(
    pubkeys: &[&'static str],
    data: &[u8],
    signature_base64: &str,
) -> Result<VerifiedSignature, String> {
    let sig_text = decode_base64_text(signature_base64).map_err(|e| format!("Couldn't decode signature: {e}"))?;
    let signature = Signature::decode(&sig_text).map_err(|e| format!("Couldn't parse signature: {e}"))?;

    let mut refusals = Vec::with_capacity(pubkeys.len());
    for pubkey in pubkeys {
        let pubkey_text = decode_base64_text(pubkey).map_err(|e| format!("Couldn't decode public key: {e}"))?;
        let public_key = PublicKey::decode(&pubkey_text).map_err(|e| format!("Couldn't parse public key: {e}"))?;
        // allow_legacy=true matches Tauri's behavior. A key whose ID isn't the signature's refuses
        // it up front, so trying each key costs one real check, not two.
        match public_key.verify(data, &signature, true) {
            // Only now is the trusted comment worth reading: `verify` checked the global
            // signature that covers it.
            Ok(()) => {
                return Ok(VerifiedSignature {
                    pubkey: pubkey.trim(),
                    signed_version: signed_version(signature.trusted_comment()).map(str::to_string),
                });
            }
            Err(e) => refusals.push(e.to_string()),
        }
    }
    Err(format!(
        "Signature verification didn't pass against any trusted key: {}",
        refusals.join("; ")
    ))
}

/// The `version:` field of a trusted comment, which the Tauri signer writes as tab-separated
/// `key:value` pairs (`timestamp:1794957084\tfile:Cmdr_0.53.0_x64-setup.exe\tversion:0.53.0`).
fn signed_version(trusted_comment: &str) -> Option<&str> {
    trusted_comment
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .filter(|version| !version.is_empty())
}

/// Decodes a base64 string into a UTF-8 string (the inner minisign text format).
fn decode_base64_text(b64: &str) -> Result<String, String> {
    let bytes = BASE64_STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("base64 decode: {e}"))?;
    String::from_utf8(bytes).map_err(|e| format!("UTF-8 decode: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_PUB: &str = include_str!("testdata/fixture.pub");
    const PAYLOAD: &[u8] = include_bytes!("testdata/payload.txt");
    const PAYLOAD_SIG: &str = include_str!("testdata/payload.txt.sig");
    /// A second throwaway key, whose signatures carry `version:0.53.0` (`tauri signer sign
    /// --app-version 0.53.0`).
    const VERSIONED_PUB: &str = include_str!("testdata/versioned-fixture.pub");
    const INSTALLER: &[u8] = include_bytes!("testdata/installer.bin");
    const INSTALLER_SIG: &str = include_str!("testdata/installer.bin.sig");

    /// A key's ID the way minisign prints it (little-endian hex), read from the key itself rather
    /// than from its untrusted comment.
    fn key_id(pubkey_base64: &str) -> String {
        let text = decode_base64_text(pubkey_base64).unwrap();
        let body = BASE64_STANDARD.decode(text.lines().nth(1).unwrap()).unwrap();
        body[2..10].iter().rev().map(|b| format!("{b:02X}")).collect()
    }

    #[test]
    fn the_trusted_keys_are_the_main_key_then_the_backup() {
        let ids: Vec<String> = TRUSTED_PUBKEYS.iter().map(|k| key_id(k)).collect();
        assert_eq!(ids, ["A2601F36BB168C0A", "B72B6B32B7D71B58"]);
        for key in TRUSTED_PUBKEYS {
            assert!(PublicKey::decode(&decode_base64_text(key).unwrap()).is_ok());
        }
    }

    /// The bundler reads the main key from `tauri.conf.json`; the updater must trust the same one.
    #[test]
    fn the_main_key_is_the_one_in_tauri_conf() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(conf["plugins"]["updater"]["pubkey"].as_str(), Some(MAIN_PUBKEY_BASE64));
    }

    #[test]
    fn a_signature_from_any_trusted_key_verifies_and_names_that_key() {
        let verified = verify_with(&[FIXTURE_PUB], PAYLOAD, PAYLOAD_SIG).expect("verifies");
        assert_eq!(verified.pubkey, FIXTURE_PUB.trim());
        // The signing key in second place, as the backup sits.
        let verified = verify_with(&[MAIN_PUBKEY_BASE64, FIXTURE_PUB], PAYLOAD, PAYLOAD_SIG).expect("verifies");
        assert_eq!(verified.pubkey, FIXTURE_PUB.trim());
    }

    /// The macOS archives are signed without `--app-version`, and macOS binds the version through
    /// the bundle's `Info.plist` instead, so a signature with no version still verifies.
    #[test]
    fn a_signature_without_a_version_verifies_and_says_so() {
        let verified = verify_with(&[FIXTURE_PUB], PAYLOAD, PAYLOAD_SIG).expect("verifies");
        assert_eq!(verified.signed_version, None);
    }

    #[test]
    fn a_signature_made_with_app_version_names_it() {
        let verified = verify_with(&[VERSIONED_PUB], INSTALLER, INSTALLER_SIG).expect("verifies");
        assert_eq!(verified.signed_version.as_deref(), Some("0.53.0"));
    }

    #[test]
    fn the_signed_version_is_read_from_its_own_field_only() {
        assert_eq!(signed_version("timestamp:1\tfile:a\tversion:0.53.0"), Some("0.53.0"));
        assert_eq!(signed_version("timestamp:1\tfile:Cmdr_0.53.0_x64-setup.exe"), None);
        assert_eq!(signed_version("timestamp:1\tversion:"), None);
    }

    #[test]
    fn a_signature_from_an_untrusted_key_is_refused() {
        assert!(verify_with(&TRUSTED_PUBKEYS, PAYLOAD, PAYLOAD_SIG).is_err());
        assert!(verify(PAYLOAD, PAYLOAD_SIG).is_err());
    }

    #[test]
    fn a_changed_payload_is_refused() {
        assert!(verify_with(&[FIXTURE_PUB], b"Cmdr updater test payload, edited\n", PAYLOAD_SIG).is_err());
    }

    #[test]
    fn garbage_is_refused() {
        for sig in ["", "%%%", &BASE64_STANDARD.encode("one line only\n")] {
            assert!(verify_with(&[FIXTURE_PUB], PAYLOAD, sig).is_err(), "{sig:?}");
        }
    }
}
