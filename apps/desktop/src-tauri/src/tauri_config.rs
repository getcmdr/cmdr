//! Guards over the Tauri config split: `tauri.conf.json` plus one `tauri.<os>.conf.json` per
//! platform, which Tauri merges as a JSON Merge Patch (RFC 7396). This module is test-only: the
//! files are data Tauri reads at build time. Layout and why: `DETAILS.md` § "Tauri config per
//! platform".

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value};

    const SHARED: &str = include_str!("../tauri.conf.json");
    const MACOS: &str = include_str!("../tauri.macos.conf.json");
    const LINUX: &str = include_str!("../tauri.linux.conf.json");
    const WINDOWS: &str = include_str!("../tauri.windows.conf.json");

    /// The main-window keys only macOS reads: tauri-runtime-wry applies them under
    /// `cfg(target_os = "macos")`, and wry reads `acceptFirstMouse` only in its WKWebView.
    const MACOS_ONLY_WINDOW_KEYS: [&str; 4] = [
        "titleBarStyle",
        "trafficLightPosition",
        "hiddenTitle",
        "acceptFirstMouse",
    ];

    fn parse(source: &str) -> Value {
        serde_json::from_str(source).expect("a Tauri config file is JSON")
    }

    fn main_window(config: &Value) -> &Map<String, Value> {
        config["app"]["windows"][0]
            .as_object()
            .expect("the config names the main window first in `app.windows`")
    }

    /// A merge patch REPLACES arrays, so the macOS file has to restate the whole main window.
    /// This keeps that copy equal to the shared one apart from the macOS chrome.
    #[test]
    fn the_macos_main_window_is_the_shared_one_plus_only_its_chrome() {
        let shared = parse(SHARED);
        let macos = parse(MACOS);
        assert_eq!(
            shared["app"]["windows"].as_array().map(Vec::len),
            Some(1),
            "one main window in tauri.conf.json"
        );
        assert_eq!(
            macos["app"]["windows"].as_array().map(Vec::len),
            Some(1),
            "one main window in tauri.macos.conf.json"
        );

        let mut macos_window = main_window(&macos).clone();
        for key in MACOS_ONLY_WINDOW_KEYS {
            assert!(
                macos_window.remove(key).is_some(),
                "tauri.macos.conf.json's main window sets `{key}`"
            );
            assert!(
                !main_window(&shared).contains_key(key),
                "`{key}` is macOS-only, so it belongs in tauri.macos.conf.json, not tauri.conf.json"
            );
        }
        assert_eq!(
            &macos_window,
            main_window(&shared),
            "tauri.macos.conf.json's main window drifted from tauri.conf.json's"
        );
    }

    /// Off macOS nothing restates the window list, so Linux and Windows get the shared window.
    #[test]
    fn only_macos_overrides_the_window_list() {
        for (file, source) in [("tauri.linux.conf.json", LINUX), ("tauri.windows.conf.json", WINDOWS)] {
            assert!(
                parse(source)["app"]["windows"].is_null(),
                "{file} must not restate `app.windows`"
            );
        }
    }

    /// Each platform names its own installers, and the shared file names none.
    #[test]
    fn every_platform_lists_its_bundle_targets() {
        assert!(
            parse(SHARED)["bundle"]["targets"].is_null(),
            "`bundle.targets` is per platform, never in tauri.conf.json"
        );
        for (file, source) in [
            ("tauri.macos.conf.json", MACOS),
            ("tauri.linux.conf.json", LINUX),
            ("tauri.windows.conf.json", WINDOWS),
        ] {
            assert!(
                parse(source)["bundle"]["targets"].is_array(),
                "{file} must list `bundle.targets`"
            );
        }
    }

    /// WebView2 serves a custom scheme as `http://<scheme>.localhost`, not `<scheme>://localhost`
    /// (Tauri's `protocol::origin`, which `convertFileSrc` follows), so Windows restates the CSP
    /// with that origin where the shared one names the scheme. This keeps the copy equal to the
    /// shared CSP apart from that swap, and macOS and Linux on the shared one.
    #[test]
    fn the_windows_csp_is_the_shared_one_with_the_localhost_origin() {
        let scheme = crate::file_viewer::media_protocol::SCHEME;
        let csp = |source: &str| parse(source)["app"]["security"]["csp"].as_str().map(str::to_string);
        let shared = csp(SHARED).expect("tauri.conf.json sets `app.security.csp`");
        assert!(
            shared.contains(&format!("{scheme}:")),
            "the shared CSP allows `{scheme}:`"
        );
        assert_eq!(
            csp(WINDOWS),
            Some(shared.replace(&format!("{scheme}:"), &format!("http://{scheme}.localhost"))),
            "tauri.windows.conf.json's CSP drifted from tauri.conf.json's"
        );
        for (file, source) in [("tauri.macos.conf.json", MACOS), ("tauri.linux.conf.json", LINUX)] {
            assert!(csp(source).is_none(), "{file} must not restate the CSP");
        }
        // `useHttpsScheme` would move the origin to `https://`, past the CSP above.
        for (file, source) in [("tauri.conf.json", SHARED), ("tauri.windows.conf.json", WINDOWS)] {
            let windows = parse(source)["app"]["windows"].clone();
            assert!(
                windows
                    .as_array()
                    .is_none_or(|list| list.iter().all(|w| w.get("useHttpsScheme").is_none())),
                "`useHttpsScheme` is on in {file}: move the Windows CSP to `https://` with it"
            );
        }
    }

    /// The NSIS installer's updater signature has to name its version (`--app-version`), which
    /// the bundler's own signing never writes, so the release signs it in a step of its own and
    /// the Windows bundle makes no updater artifact (a key-less local `tauri build` then bundles
    /// too). macOS keeps the bundler's archive and signature.
    #[test]
    fn windows_leaves_updater_signing_to_the_release() {
        assert_eq!(parse(SHARED)["bundle"]["createUpdaterArtifacts"], Value::Bool(true));
        assert_eq!(parse(WINDOWS)["bundle"]["createUpdaterArtifacts"], Value::Bool(false));
        assert_eq!(
            parse(WINDOWS)["bundle"]["publisher"],
            Value::from("Rymdskottkärra AB"),
            "the installer and Apps & features name the company, not the identifier's second part"
        );
    }

    /// tauri-build fails the build when the `tauri` dependency's features disagree with the
    /// effective config's (`tauri-build` `manifest::check`), and Cargo.toml turns
    /// `macos-private-api` on for every target. So the key stays shared.
    #[test]
    fn macos_private_api_stays_in_the_shared_config() {
        assert_eq!(parse(SHARED)["app"]["macOSPrivateApi"], Value::Bool(true));
        for (file, source) in [
            ("tauri.macos.conf.json", MACOS),
            ("tauri.linux.conf.json", LINUX),
            ("tauri.windows.conf.json", WINDOWS),
        ] {
            assert!(
                parse(source)["app"]["macOSPrivateApi"].is_null(),
                "{file} must not set `macOSPrivateApi`"
            );
        }
    }
}
