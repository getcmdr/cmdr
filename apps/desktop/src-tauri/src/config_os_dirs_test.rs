//! The OS data and cache dirs, held to the case table `os_app_dirs_cases.json` that
//! `scripts/instance-id.test.ts` runs too, so the dev wrapper's `CMDR_DATA_DIR` /
//! `CMDR_CACHE_DIR` and the backend's own OS defaults can't drift apart.
//!
//! The backend doesn't compute these: Tauri's `app_data_dir()` and [`super::standalone_app_data_dir`]
//! join the identifier onto `dirs::data_dir()`, and [`super::app_cache_dir`] onto
//! `dirs::cache_dir()`. So the table runs against [`model`], a model of what `dirs` reads on
//! each OS, and [`the_model_is_what_dirs_answers_on_this_host`] pins the model to the real
//! `dirs` on whatever OS runs the tests: macOS locally, macOS and Linux in CI. Nothing runs the
//! Rust tests on Windows yet (the Windows lane only compiles), so the Windows rows are held to
//! the model alone until one does.

use std::collections::HashMap;

use super::{BUNDLE_ID, os_cache_dir_for};

#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum Os {
    Macos,
    Linux,
    Windows,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    os: Os,
    home: String,
    env: HashMap<String, String>,
    identifier: String,
    data_dir: String,
    cache_dir: String,
}

#[derive(serde::Deserialize)]
struct Table {
    cases: Vec<Case>,
}

fn table() -> Table {
    serde_json::from_str(include_str!("os_app_dirs_cases.json")).expect("the shared cases parse")
}

/// Joins path segments with the target OS's separator, so a Windows case reads the same on a Mac.
fn join(os: Os, parts: &[&str]) -> String {
    parts.join(if os == Os::Windows { "\\" } else { "/" })
}

/// What `dirs` 7 reads per OS, as `(data_dir, cache_dir)` bases. macOS: fixed folders under the
/// home. Linux: an absolute XDG variable, else the XDG default under the home. Windows: the
/// Roaming and Local known folders, which `%APPDATA%` and `%LOCALAPPDATA%` name.
fn model(os: Os, home: &str, env: &HashMap<String, String>) -> (String, String) {
    let var = |key: &str| env.get(key).map(String::as_str);
    match os {
        Os::Macos => (
            join(os, &[home, "Library", "Application Support"]),
            join(os, &[home, "Library", "Caches"]),
        ),
        Os::Linux => {
            let xdg = |key: &str, fallback: &str| match var(key) {
                Some(value) if value.starts_with('/') => value.to_string(),
                _ => join(os, &[home, fallback]),
            };
            (xdg("XDG_DATA_HOME", ".local/share"), xdg("XDG_CACHE_HOME", ".cache"))
        }
        Os::Windows => {
            let known = |key: &str, fallback: &str| match var(key) {
                Some(value) if !value.is_empty() => value.to_string(),
                _ => join(os, &[home, "AppData", fallback]),
            };
            (known("APPDATA", "Roaming"), known("LOCALAPPDATA", "Local"))
        }
    }
}

#[test]
fn every_shared_case_holds() {
    let cases = table().cases;
    for os in [Os::Macos, Os::Linux, Os::Windows] {
        assert!(cases.iter().any(|c| c.os == os), "the table has no {os:?} case");
    }
    for case in cases {
        let (data, cache) = model(case.os, &case.home, &case.env);
        assert_eq!(
            join(case.os, &[&data, &case.identifier]),
            case.data_dir,
            "{}",
            case.name
        );
        assert_eq!(
            join(case.os, &[&cache, &case.identifier]),
            case.cache_dir,
            "{}",
            case.name
        );
    }
}

#[test]
fn the_model_is_what_dirs_answers_on_this_host() {
    let os = if cfg!(target_os = "macos") {
        Os::Macos
    } else if cfg!(windows) {
        Os::Windows
    } else {
        Os::Linux
    };
    let home = dirs::home_dir().expect("the test host has a home dir");
    let home = home.to_str().expect("the test host's home dir is UTF-8");
    let env: HashMap<String, String> = ["XDG_DATA_HOME", "XDG_CACHE_HOME", "APPDATA", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_string(), value)))
        .collect();
    let (data, cache) = model(os, home, &env);

    let real_data = dirs::data_dir().expect("the test host has a data dir");
    let real_cache = dirs::cache_dir().expect("the test host has a cache dir");
    assert_eq!(
        real_data.to_str(),
        Some(data.as_str()),
        "dirs::data_dir() left the model"
    );
    assert_eq!(
        real_cache.to_str(),
        Some(cache.as_str()),
        "dirs::cache_dir() left the model"
    );
    // The production join onto the cache base is the table's join too.
    assert_eq!(
        os_cache_dir_for(&real_cache, BUNDLE_ID).to_str(),
        Some(join(os, &[&cache, BUNDLE_ID]).as_str())
    );
}
