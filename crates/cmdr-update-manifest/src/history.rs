//! Every `latest.json` reader that ever shipped, pinned as it parsed, so a manifest the release
//! writer produces is checked against the installs still out there, not only against today's
//! build. Fields may be added; none may be removed or retyped, and each published manifest shape
//! (`history/*.json`) must still read in every one.
//!
//! - [`Reader::TauriPlugin`]: Cmdr 0.7 and earlier checked through `tauri-plugin-updater` 2,
//!   whose `RemoteRelease` reads `version` (a leading `v` allowed), `notes` and `pub_date` as
//!   optional strings with `pub_date` RFC 3339, and EVERY platform entry's `url` as a `Url`: one
//!   bad entry for any platform fails the whole check. The app pins this copy against the real
//!   plugin type (`updater/manifest.rs`, `the_plugin_reads_what_its_mirror_reads`).
//! - [`Reader::Cmdr0_8`]: Cmdr 0.8.0 to 0.61.x (`updater/manifest.rs` from `190a637767` until the
//!   advisory entry): `version` and `platforms` with every entry a `{ url, signature }` of strings,
//!   strict per entry, so again one bad entry anywhere fails every install.
//! - [`Reader::Current`]: this crate's [`crate::parse`].
//!
//! Each reads the macOS keys (`darwin-aarch64`, `darwin-x86_64`): the only installs they shipped to.

use serde::Deserialize;
use std::collections::HashMap;

/// A reader that shipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    TauriPlugin,
    Cmdr0_8,
    Current,
}

impl Reader {
    pub const ALL: [Self; 3] = [Self::TauriPlugin, Self::Cmdr0_8, Self::Current];

    /// Which installs read with it.
    pub fn label(self) -> &'static str {
        match self {
            Self::TauriPlugin => "Cmdr 0.7 and earlier (tauri-plugin-updater 2)",
            Self::Cmdr0_8 => "Cmdr 0.8.0 to 0.61.x",
            Self::Current => "this build",
        }
    }
}

/// The macOS keys every reader reads, by arch.
pub(crate) const LEGACY_KEYS: [&str; 2] = ["darwin-aarch64", "darwin-x86_64"];

/// What one reader made of a manifest: its version and the entries it would download from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub version: semver::Version,
    pub urls: HashMap<String, String>,
}

/// Reads `body` as `reader` did, or says why that reader refused it.
pub fn read(reader: Reader, body: &[u8]) -> Result<Reading, String> {
    match reader {
        Reader::TauriPlugin => read_tauri_plugin(body),
        Reader::Cmdr0_8 => read_cmdr_0_8(body),
        Reader::Current => {
            let m = crate::parse(body).map_err(|e| e.to_string())?;
            let mut urls = HashMap::new();
            for (key, entry) in m.platforms {
                urls.insert(key.clone(), entry.map_err(|e| format!("{key}: {e}"))?.url);
            }
            Ok(Reading {
                version: m.version,
                urls,
            })
        }
    }
}

#[derive(Deserialize)]
struct TauriPluginRelease {
    #[serde(alias = "name")]
    version: String,
    // Read for their types alone, as the plugin did.
    #[expect(dead_code, reason = "the plugin read it; the type is what's pinned")]
    notes: Option<String>,
    pub_date: Option<String>,
    platforms: Option<HashMap<String, TauriPluginPlatform>>,
    url: Option<url::Url>,
    #[expect(dead_code, reason = "the plugin read it; the type is what's pinned")]
    signature: Option<String>,
}

#[derive(Deserialize)]
struct TauriPluginPlatform {
    #[expect(dead_code, reason = "the plugin read it; the type is what's pinned")]
    signature: String,
    url: url::Url,
}

fn read_tauri_plugin(body: &[u8]) -> Result<Reading, String> {
    let release: TauriPluginRelease = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let version = semver::Version::parse(release.version.trim_start_matches('v')).map_err(|e| e.to_string())?;
    if let Some(date) = &release.pub_date {
        time::OffsetDateTime::parse(date, &time::format_description::well_known::Rfc3339)
            .map_err(|e| format!("invalid value for `pub_date`: {e}"))?;
    }
    let urls = match release.platforms {
        Some(platforms) => platforms.into_iter().map(|(k, p)| (k, p.url.to_string())).collect(),
        None => {
            let url = release
                .url
                .ok_or("the `url` field was not set on the updater response")?;
            LEGACY_KEYS
                .iter()
                .map(|k| ((*k).to_string(), url.to_string()))
                .collect()
        }
    };
    Ok(Reading { version, urls })
}

#[derive(Deserialize)]
struct Cmdr0_8Manifest {
    version: String,
    platforms: HashMap<String, Cmdr0_8Entry>,
}

#[derive(Deserialize)]
struct Cmdr0_8Entry {
    url: String,
    #[expect(dead_code, reason = "0.8 read it; the type is what's pinned")]
    signature: String,
}

fn read_cmdr_0_8(body: &[u8]) -> Result<Reading, String> {
    let m: Cmdr0_8Manifest = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    // 0.8 parsed the version only when comparing, and read a non-semver one as "no update".
    let version = semver::Version::parse(&m.version).map_err(|e| format!("no update: {e}"))?;
    Ok(Reading {
        version,
        urls: m.platforms.into_iter().map(|(k, e)| (k, e.url)).collect(),
    })
}

/// The release gate: every reader reads `body` and finds an update in it for each install it
/// serves (the macOS keys for the old readers, every key for this build), with nothing this build
/// would drop. Answers one line per reader, or every problem found.
pub fn gate(body: &[u8]) -> Result<Vec<String>, Vec<String>> {
    let mut report = Vec::new();
    let mut problems = Vec::new();
    for reader in Reader::ALL {
        match read(reader, body) {
            Err(e) => problems.push(format!("{} can't read it: {e}", reader.label())),
            Ok(reading) => {
                let keys: Vec<String> = match reader {
                    Reader::Current => reading.urls.keys().cloned().collect(),
                    _ => LEGACY_KEYS.iter().map(|k| (*k).to_string()).collect(),
                };
                let missing: Vec<&String> = keys.iter().filter(|k| !reading.urls.contains_key(*k)).collect();
                if missing.is_empty() {
                    let mut keys = keys;
                    keys.sort_unstable();
                    report.push(format!(
                        "{}: {} for {}",
                        reader.label(),
                        reading.version,
                        keys.join(", ")
                    ));
                } else {
                    problems.push(format!("{} finds no entry for {missing:?}", reader.label()));
                }
            }
        }
    }
    // This build drops a broken advisory entry quietly; the gate doesn't.
    if let Ok(text) = std::str::from_utf8(body)
        && let Ok(serde_json::Value::Object(top)) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(entry) = top.get("securityAdvisories")
        && let Err(e) = crate::read_entry(entry.clone())
    {
        problems.push(format!("this build would drop its securityAdvisories entry: {e}"));
    }
    if problems.is_empty() { Ok(report) } else { Err(problems) }
}
