//! Parses `latest.json` from the update server and determines whether an update is available.

use serde::Deserialize;
use std::collections::HashMap;

/// The full `latest.json` manifest served by the update server.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub version: String,
    pub platforms: HashMap<String, PlatformEntry>,
    /// The release's advisory list as a signed release asset, for the installs that can't read it
    /// out of the update itself (Windows and Linux, `advisories::fetch_signed_list`). Optional: a
    /// manifest without it means no security notice, never a failed check.
    #[serde(default)]
    pub security_advisories: Option<PlatformEntry>,
}

/// Per-platform entry in the manifest: download URL and minisign signature.
#[derive(Debug, Clone, Deserialize)]
pub struct PlatformEntry {
    pub url: String,
    pub signature: String,
}

/// A newer release for this platform: what a check offers and `download_update` fetches. Stays in
/// the backend; the frontend only ever sees the version.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: semver::Version,
    /// The `latest.json` key the entry came from, which the plugin hand-off asks for by name.
    pub platform_key: String,
    pub url: String,
    pub signature: String,
    /// The signed advisory list the manifest names, if any.
    pub security_advisories: Option<PlatformEntry>,
}

/// Checks whether the remote version is newer than the current app version, reading the first of
/// `platform_keys` the manifest carries (`InstallKind::platform_keys`).
/// Returns `Some(UpdateInfo)` if an update is available, `None` otherwise.
///
/// Three of the four `None` paths mean something is wrong with the manifest rather than "you're up
/// to date", and the caller can't tell them apart: it gets a bare `None` either way, which the
/// frontend renders as "Cmdr is up to date". So each one warns. An install that quietly never
/// updates because the manifest lost its platform entry is otherwise invisible from both ends,
/// and only the "up to date" path is routine enough to stay at debug.
pub fn check_manifest(
    manifest: &UpdateManifest,
    current_version: &str,
    platform_keys: &[String],
) -> Option<UpdateInfo> {
    let current = match semver::Version::parse(current_version) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Can't compare versions: this build's own version {current_version} isn't semver: {e}");
            return None;
        }
    };
    let latest = match semver::Version::parse(&manifest.version) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "Can't compare versions: manifest version {} isn't semver: {e}",
                manifest.version
            );
            return None;
        }
    };

    if latest <= current {
        log::debug!("No update available (current={current}, remote={latest})");
        return None;
    }

    let Some((platform, entry)) = platform_keys
        .iter()
        .find_map(|key| manifest.platforms.get(key).map(|entry| (key, entry)))
    else {
        let mut offered: Vec<&str> = manifest.platforms.keys().map(String::as_str).collect();
        offered.sort_unstable();
        log::warn!(
            "Update {latest} is out but has no {} build, so this install stays on {current} (manifest offers: {})",
            platform_keys.join(" or "), // allowed-log-private: our own manifest keys
            offered.join(", ")
        );
        return None;
    };

    log::info!("Update available: {current} -> {latest} (platform={platform})");
    Some(UpdateInfo {
        version: latest,
        platform_key: platform.clone(),
        url: entry.url.clone(),
        signature: entry.signature.clone(),
        security_advisories: manifest.security_advisories.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| (*key).to_string()).collect()
    }

    fn manifest(version: &str, platforms: &[&str]) -> UpdateManifest {
        UpdateManifest {
            version: version.to_string(),
            platforms: platforms
                .iter()
                .map(|key| {
                    (
                        (*key).to_string(),
                        PlatformEntry {
                            url: format!("https://example.invalid/{key}.tar.gz"),
                            signature: format!("sig-{key}"),
                        },
                    )
                })
                .collect(),
            security_advisories: None,
        }
    }

    #[test]
    fn newer_version_with_this_platform_is_an_update() {
        let m = manifest("0.41.0", &["darwin-aarch64", "darwin-x86_64", "darwin-universal"]);
        let info = check_manifest(&m, "0.40.0", &keys(&["darwin-x86_64"])).expect("0.41.0 is newer than 0.40.0");
        assert_eq!(info.version.to_string(), "0.41.0");
        assert_eq!(info.platform_key, "darwin-x86_64");
        assert_eq!(info.url, "https://example.invalid/darwin-x86_64.tar.gz");
        assert_eq!(info.signature, "sig-darwin-x86_64");
    }

    /// The installer-specific key wins over the bare one, the order the Tauri plugin reads them in.
    #[test]
    fn the_first_key_the_manifest_carries_wins() {
        let m = manifest("0.41.0", &["windows-x86_64", "windows-x86_64-nsis"]);
        let wanted = keys(&["windows-x86_64-nsis", "windows-x86_64"]);
        assert_eq!(
            check_manifest(&m, "0.40.0", &wanted).map(|info| info.platform_key),
            Some("windows-x86_64-nsis".to_string())
        );
        let bare_only = manifest("0.41.0", &["windows-x86_64"]);
        assert_eq!(
            check_manifest(&bare_only, "0.40.0", &wanted).map(|info| info.platform_key),
            Some("windows-x86_64".to_string())
        );
    }

    #[test]
    fn same_version_is_not_an_update() {
        let m = manifest("0.40.0", &["darwin-aarch64", "darwin-x86_64"]);
        assert!(check_manifest(&m, "0.40.0", &keys(&["darwin-aarch64"])).is_none());
    }

    #[test]
    fn older_remote_version_is_not_an_update() {
        let m = manifest("0.39.0", &["darwin-aarch64", "darwin-x86_64"]);
        assert!(check_manifest(&m, "0.40.0", &keys(&["darwin-aarch64"])).is_none());
    }

    /// A manifest that lost this build's platform entry reads to the caller exactly like "you're
    /// up to date", so the install would sit on an old version indefinitely with nothing said.
    /// The `None` is the contract; the warn beside it is what makes the case diagnosable.
    #[test]
    fn a_newer_version_with_no_entry_for_this_platform_is_not_an_update() {
        let m = manifest("0.41.0", &["darwin-aarch64", "darwin-x86_64"]);
        assert!(check_manifest(&m, "0.40.0", &keys(&["windows-x86_64-nsis", "windows-x86_64"])).is_none());
    }

    #[test]
    fn a_non_semver_manifest_version_is_not_an_update() {
        let m = manifest("latest", &["darwin-aarch64", "darwin-x86_64"]);
        assert!(check_manifest(&m, "0.40.0", &keys(&["darwin-aarch64"])).is_none());
    }

    #[test]
    fn a_prerelease_is_older_than_its_release() {
        let m = manifest("0.41.0-rc1", &["darwin-aarch64", "darwin-x86_64"]);
        assert!(
            check_manifest(&m, "0.41.0", &keys(&["darwin-aarch64"])).is_none(),
            "semver orders a prerelease below its release, so 0.41.0 must not downgrade"
        );
    }

    /// Today's manifests carry no advisory entry, and the macOS check must read them unchanged.
    #[test]
    fn a_manifest_with_or_without_the_advisory_entry_parses() {
        let without: UpdateManifest = serde_json::from_value(serde_json::json!({
            "version": "0.41.0",
            "notes": "ignored",
            "platforms": { "darwin-aarch64": { "url": "https://example.invalid/a", "signature": "s" } }
        }))
        .expect("parses");
        assert!(without.security_advisories.is_none());
        let with: UpdateManifest = serde_json::from_value(serde_json::json!({
            "version": "0.41.0",
            "platforms": {},
            "securityAdvisories": { "url": "https://example.invalid/security-advisories.json", "signature": "s" }
        }))
        .expect("parses");
        assert_eq!(
            with.security_advisories.map(|entry| entry.url).as_deref(),
            Some("https://example.invalid/security-advisories.json")
        );
    }
}
