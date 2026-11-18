//! What every webview learns about the platform before any of its scripts run: which OS, what it
//! can do, and how its own apps behave. `tauri_builder.rs` registers it as a plugin's init script,
//! which Tauri runs in every webview the app creates. See `apps/desktop/src/lib/platform/DETAILS.md`.

use super::{PlatformCapabilities, PlatformConventions, capabilities, conventions};

/// The platform the frontend's `#lib/platform` reports: Rust's own name for the target OS, which is
/// one of the three values its `Platform` type allows (`macos`, `linux`, `windows`) because the
/// crate root refuses every other target (`cmdr_host`, `docs/guides/platform-seams.md`).
const FRONTEND_PLATFORM: &str = std::env::consts::OS;

/// Defines `window.__CMDR_PLATFORM__`, `window.__CMDR_CAPABILITIES__`, and
/// `window.__CMDR_CONVENTIONS__`, all read-only (the two objects frozen), so nothing in the page can
/// change its mind later.
pub(crate) fn init_script() -> String {
    script_for(FRONTEND_PLATFORM, &capabilities(), &conventions())
}

fn script_for(platform: &str, capabilities: &PlatformCapabilities, conventions: &PlatformConventions) -> String {
    // Structs of bools and unit enums always serialize, and their JSON is a valid JS object
    // literal as is.
    let capabilities = serde_json::to_string(capabilities).expect("a struct of bools serializes");
    let conventions = serde_json::to_string(conventions).expect("a struct of unit enums serializes");
    format!(
        "Object.defineProperty(window, '__CMDR_PLATFORM__', {{ value: '{platform}' }});\
         Object.defineProperty(window, '__CMDR_CAPABILITIES__', {{ value: Object.freeze({capabilities}) }});\
         Object.defineProperty(window, '__CMDR_CONVENTIONS__', {{ value: Object.freeze({conventions}) }});"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_running_os() {
        assert!(["macos", "linux", "windows"].contains(&FRONTEND_PLATFORM));
        assert_eq!(FRONTEND_PLATFORM, std::env::consts::OS);
        assert!(init_script().starts_with(&format!(
            "Object.defineProperty(window, '__CMDR_PLATFORM__', {{ value: '{}' }});",
            std::env::consts::OS
        )));
    }

    #[test]
    fn hands_over_this_os_capabilities_in_the_frontend_spelling() {
        let script = init_script();
        let json = serde_json::to_string(&capabilities()).expect("serializes");
        assert!(script.contains(&format!(
            "Object.defineProperty(window, '__CMDR_CAPABILITIES__', {{ value: Object.freeze({json}) }});"
        )));
        // camelCase on the wire, as the generated TS type spells it.
        assert!(json.contains("\"quickLook\":"));
        assert!(json.contains("\"overlayTitleBar\":"));
        assert!(!json.contains('_'));
    }

    #[test]
    fn hands_over_this_desktops_conventions_in_the_frontend_spelling() {
        let script = init_script();
        let json = serde_json::to_string(&conventions()).expect("serializes");
        assert!(script.ends_with(&format!(
            "Object.defineProperty(window, '__CMDR_CONVENTIONS__', {{ value: Object.freeze({json}) }});"
        )));
        // camelCase keys and values, as the generated TS types spell them.
        assert!(json.contains("\"dialogButtonOrder\":"));
        assert!(json.contains("\"screenReaders\":["));
        assert!(!json.contains('_'));
    }
}
