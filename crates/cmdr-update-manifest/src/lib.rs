//! `latest.json`, the update manifest every installed Cmdr polls: the app's reader, and every
//! reader that ever shipped (`history`), which the release gate (`cmdr-check-update-manifest`)
//! runs over a candidate before it's published.
//!
//! The reader is lenient where the data is optional and strict where it's required: an entry for
//! another platform, or an advisory entry, that doesn't parse is dropped with a warn and never
//! fails the check; a version that isn't semver, or a broken entry for THIS install, refuses the
//! manifest ([`ManifestRefusal`]), which the app reports as an answer it couldn't read rather than
//! "you're up to date". `DETAILS.md` § The reader's contract.

// The workspace lint set comes from `Cargo.toml`'s `lints.workspace = true`; this one is judged
// per compilation unit, so it stays here.
#![warn(unused_crate_dependencies)]

use serde::Deserialize;
use std::collections::HashMap;

pub mod history;

#[cfg(test)]
mod tests;

/// The most bytes a manifest may weigh. Published ones are about 10 KB; the release writer refuses
/// anything over a quarter of this (`scripts/release-assets/validate.go`), so the cap only ever
/// meets something that isn't a manifest.
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;

/// The wire shape, every member this reader names. `camelCase` names every field, and the writer
/// emits `pub_date` in snake_case, so a new field must match the key the writer really writes:
/// `every_field_the_reader_names_is_one_the_writer_writes` checks it against the writer's output.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Wire {
    pub(crate) version: String,
    /// Read entry by entry ([`UpdateManifest::platforms`]), so one entry this build can't read
    /// costs only that platform.
    pub(crate) platforms: HashMap<String, serde_json::Value>,
    /// The release's advisory list as a signed release asset, for the installs that can't read it
    /// out of the update itself (Windows and Linux). Optional, and read leniently.
    #[serde(default)]
    pub(crate) security_advisories: Option<serde_json::Value>,
}

/// A parsed manifest.
#[derive(Debug, Clone)]
pub struct UpdateManifest {
    pub version: semver::Version,
    /// Each platform's entry, or why it doesn't read.
    pub platforms: HashMap<String, Result<PlatformEntry, String>>,
    /// The signed advisory list the manifest names, if any (a broken entry reads as none).
    pub security_advisories: Option<PlatformEntry>,
}

/// A platform's download URL and minisign signature (and the advisory list's, same shape).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlatformEntry {
    pub url: String,
    pub signature: String,
}

/// A newer release for this platform: what a check offers and the download fetches.
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

/// Why a body isn't a manifest this build can act on. Each means Cmdr's server and this build
/// disagree (or something else answered), so the app reports it; none is "you're up to date".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestRefusal {
    TooLarge {
        bytes: usize,
    },
    NotUtf8,
    /// A 200 whose body opens like markup: a captive portal, a CDN error page.
    LooksLikeHtml,
    NotJson {
        detail: String,
    },
    VersionNotSemver {
        version: String,
    },
    /// This install's own entry doesn't read, or names no usable URL or signature.
    BadEntry {
        key: String,
        detail: String,
    },
}

impl std::fmt::Display for ManifestRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes } => {
                write!(
                    f,
                    "the manifest's size ({bytes}) is over the {MAX_MANIFEST_BYTES}-byte cap"
                )
            }
            Self::NotUtf8 => f.write_str("the manifest isn't UTF-8"),
            Self::LooksLikeHtml => f.write_str("the manifest is an HTML page"),
            Self::NotJson { detail } => write!(f, "the manifest doesn't parse: {detail}"),
            Self::VersionNotSemver { version } => write!(f, "the manifest's version {version:?} isn't semver"),
            Self::BadEntry { key, detail } => write!(f, "the manifest's {key} entry is unusable: {detail}"),
        }
    }
}

impl std::error::Error for ManifestRefusal {}

/// Parses a manifest body. Bounded: a body over [`MAX_MANIFEST_BYTES`] is refused before it's read
/// as JSON. Never panics, whatever the bytes.
pub fn parse(body: &[u8]) -> Result<UpdateManifest, ManifestRefusal> {
    if body.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestRefusal::TooLarge { bytes: body.len() });
    }
    let text = std::str::from_utf8(body).map_err(|_| ManifestRefusal::NotUtf8)?;
    if text.trim_start().starts_with('<') {
        return Err(ManifestRefusal::LooksLikeHtml);
    }
    let wire: Wire = serde_json::from_str(text).map_err(|e| ManifestRefusal::NotJson { detail: e.to_string() })?;
    let version = semver::Version::parse(&wire.version).map_err(|_| ManifestRefusal::VersionNotSemver {
        version: wire.version.clone(),
    })?;
    let platforms = wire
        .platforms
        .into_iter()
        .map(|(key, value)| {
            let entry = read_entry(value);
            if let Err(e) = &entry {
                // allowed-log-private: our own manifest's key
                log::warn!("The manifest's {key} entry doesn't read, so that platform gets no update: {e}");
            }
            (key, entry)
        })
        .collect();
    let security_advisories = wire.security_advisories.and_then(|value| {
        read_entry(value)
            .inspect_err(|e| {
                log::warn!("The manifest's security advisory entry doesn't read, so it names no advisory: {e}");
            })
            .ok()
    });
    Ok(UpdateManifest {
        version,
        platforms,
        security_advisories,
    })
}

/// Reads one `{ url, signature }` entry: an absolute `https` URL and a non-empty signature.
fn read_entry(value: serde_json::Value) -> Result<PlatformEntry, String> {
    let entry: PlatformEntry = serde_json::from_value(value).map_err(|e| e.to_string())?;
    match url::Url::parse(&entry.url) {
        Ok(u) if u.scheme() == "https" && u.host().is_some() => {}
        _ => return Err(format!("{:?} isn't an absolute https URL", entry.url)),
    }
    if entry.signature.trim().is_empty() {
        return Err("its signature is empty".to_string());
    }
    Ok(entry)
}

/// Checks whether the manifest offers this install an update: a version newer than
/// `current_version`, with an entry under the first of `platform_keys` the manifest carries.
///
/// `Ok(None)` is "up to date", which also covers a manifest with no entry for this install (logged
/// at warn: a release that left this platform out is otherwise invisible from both ends). An
/// entry for this install that doesn't read is `Err`: the release meant to offer it and can't.
pub fn check_manifest(
    manifest: &UpdateManifest,
    current_version: &str,
    platform_keys: &[String],
) -> Result<Option<UpdateInfo>, ManifestRefusal> {
    let current = match semver::Version::parse(current_version) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Can't compare versions: this build's own version {current_version} isn't semver: {e}");
            return Ok(None);
        }
    };
    let latest = &manifest.version;
    if *latest <= current {
        log::debug!("No update available (current={current}, remote={latest})");
        return Ok(None);
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
        return Ok(None);
    };
    let entry = entry.as_ref().map_err(|detail| ManifestRefusal::BadEntry {
        key: platform.clone(),
        detail: detail.clone(),
    })?;

    log::info!("Update available: {current} -> {latest} (platform={platform})");
    Ok(Some(UpdateInfo {
        version: latest.clone(),
        platform_key: platform.clone(),
        url: entry.url.clone(),
        signature: entry.signature.clone(),
        security_advisories: manifest.security_advisories.clone(),
    }))
}
