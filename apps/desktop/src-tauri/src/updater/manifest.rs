//! Parses `latest.json` from the update server and determines whether an update is available.

use serde::Deserialize;
use std::collections::HashMap;

/// The full `latest.json` manifest served by the update server. `camelCase` names every field, and
/// the writer emits `pub_date` in snake_case, so a new field must match the key the writer really
/// writes: `every_field_the_reader_names_is_one_the_writer_writes` checks it against the writer's
/// golden output.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub version: String,
    pub platforms: HashMap<String, PlatformEntry>,
    /// The release's advisory list as a signed release asset, for the installs that can't read it
    /// out of the update itself (Windows and Linux, `advisories::fetch_signed_list`). Optional: a
    /// manifest without it, or with one that doesn't parse, means no security notice, never a
    /// failed check ([`lenient_advisory_entry`]).
    #[serde(default, deserialize_with = "lenient_advisory_entry")]
    pub security_advisories: Option<PlatformEntry>,
}

/// Reads `securityAdvisories` without ever failing the manifest: an entry that isn't a
/// `{ url, signature }` is dropped with a warn. The list only adds a notice (nothing refuses an
/// install over it, on any OS), so losing it costs the notice, as an unreadable list in the macOS
/// bundle does, while a strict parse would stop every install on every OS from updating.
fn lenient_advisory_entry<'de, D>(deserializer: D) -> Result<Option<PlatformEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw.and_then(|value| {
        serde_json::from_value(value)
            .inspect_err(|e| {
                log::warn!("The manifest's security advisory entry doesn't parse, so it names no advisory: {e}");
            })
            .ok()
    }))
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

    /// The release writer's own output (`scripts/release-assets`, whose `latest_test.go` pins its
    /// bytes against this file), so this side and the Go side share one fixture.
    const WRITTEN_LATEST_JSON: &str =
        include_str!("../../../../../scripts/release-assets/testdata/v0.61.0/latest.json");

    /// Fields this reader names that the writer doesn't emit yet, each with why. Shrink it as the
    /// writer catches up; ❌ never add a field here to quiet a rename mismatch.
    const NOT_WRITTEN_YET: &[&str] = &[
        // The Windows and Linux signed advisory list: no release job produces the asset or the
        // entry yet (`scripts/release-assets/DETAILS.md`, the release signing job's open work).
        "securityAdvisories",
    ];

    /// The JSON names a derived `Deserialize` asks for, after every `rename`: serde hands them to
    /// `deserialize_struct`, so a deserializer that records them and stops is a field-set probe.
    fn field_names<'de, T: Deserialize<'de>>() -> &'static [&'static str] {
        struct Probe(&'static [&'static str]);
        #[derive(Debug)]
        struct Stop;
        impl std::fmt::Display for Stop {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("probe")
            }
        }
        impl std::error::Error for Stop {}
        impl serde::de::Error for Stop {
            fn custom<M: std::fmt::Display>(_: M) -> Self {
                Stop
            }
        }
        impl<'de> serde::Deserializer<'de> for &mut Probe {
            type Error = Stop;
            fn deserialize_any<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value, Stop> {
                Err(Stop)
            }
            fn deserialize_struct<V: serde::de::Visitor<'de>>(
                self,
                _: &'static str,
                fields: &'static [&'static str],
                _: V,
            ) -> Result<V::Value, Stop> {
                self.0 = fields;
                Err(Stop)
            }
            serde::forward_to_deserialize_any! {
                bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
                option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier
                ignored_any
            }
        }
        let mut probe = Probe(&[]);
        let _ = T::deserialize(&mut probe);
        assert!(!probe.0.is_empty(), "not a struct with named fields");
        probe.0
    }

    /// `rename_all = "camelCase"` reads a snake_case Rust field under its camelCase name, so a
    /// field added for a snake_case key (the writer already emits `pub_date`) would silently never
    /// be read. Every name this reader asks for must be one the release writer writes.
    #[test]
    fn every_field_the_reader_names_is_one_the_writer_writes() {
        let written: serde_json::Value =
            serde_json::from_str(WRITTEN_LATEST_JSON).expect("the writer's output is JSON");
        let top = written.as_object().expect("an object");
        for name in field_names::<UpdateManifest>() {
            assert!(
                top.contains_key(*name) || NOT_WRITTEN_YET.contains(name),
                "`UpdateManifest` reads `{name}`, which the release writer never writes (a camelCase rename of a snake_case key?)"
            );
        }
        let entry = top["platforms"]
            .as_object()
            .and_then(|platforms| platforms.values().next())
            .and_then(serde_json::Value::as_object)
            .expect("the writer's output has a platform entry");
        for name in field_names::<PlatformEntry>() {
            assert!(
                entry.contains_key(*name),
                "`PlatformEntry` reads `{name}`, which the writer never writes"
            );
        }
        serde_json::from_str::<UpdateManifest>(WRITTEN_LATEST_JSON).expect("the reader parses the writer's output");
    }

    /// A broken advisory entry costs the notice, never the update: the list only ever adds a
    /// notice (no advisory refuses an install, on macOS either), so dropping it is the safe side.
    /// Before, it failed the whole parse, so every install on every OS stopped updating.
    #[test]
    fn a_malformed_advisory_entry_is_dropped_and_the_manifest_still_parses() {
        for broken in [
            serde_json::json!({ "url": "https://example.invalid/list.json" }),
            serde_json::json!({ "url": null, "signature": "s" }),
            serde_json::json!("https://example.invalid/list.json"),
            serde_json::json!([]),
            serde_json::json!(null),
        ] {
            let parsed: UpdateManifest = serde_json::from_value(serde_json::json!({
                "version": "0.41.0",
                "platforms": { "darwin-aarch64": { "url": "https://example.invalid/a", "signature": "s" } },
                "securityAdvisories": broken,
            }))
            .unwrap_or_else(|e| panic!("a manifest with advisory entry {broken} must still parse: {e}"));
            assert!(parsed.security_advisories.is_none(), "{broken}");
            assert!(check_manifest(&parsed, "0.40.0", &keys(&["darwin-aarch64"])).is_some());
        }
    }
}
