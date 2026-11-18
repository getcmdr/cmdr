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
    let (copy_platform, conventions) = frontend_conventions();
    script_for(FRONTEND_PLATFORM, &capabilities(), &conventions, copy_platform)
}

/// This desktop's conventions, or in a debug build the row `CMDR_MOCK_CONVENTIONS` names, plus
/// the platform a mocked row's copy reads (`None` when nothing is mocked).
fn frontend_conventions() -> (Option<&'static str>, PlatformConventions) {
    #[cfg(debug_assertions)]
    if let Some((desktop, mocked)) = conventions::mock_conventions() {
        log::info!(target: "platform", "CMDR_MOCK_CONVENTIONS: the frontend follows another desktop's conventions and copy");
        return (Some(desktop.copy_platform()), mocked);
    }
    (None, conventions())
}

/// The init script. A mocked desktop also defines `window.__CMDR_COPY_PLATFORM__`, which only the
/// copy reader follows (`#lib/intl`), so the window speaks the mocked desktop's words while
/// keymaps and capabilities stay this OS's.
fn script_for(
    platform: &str,
    capabilities: &PlatformCapabilities,
    conventions: &PlatformConventions,
    copy_platform: Option<&str>,
) -> String {
    // Structs of bools and unit enums always serialize, and their JSON is a valid JS object
    // literal as is.
    let capabilities = serde_json::to_string(capabilities).expect("a struct of bools serializes");
    let conventions = serde_json::to_string(conventions).expect("a struct of unit enums serializes");
    let copy = copy_platform
        .map(|copy| format!("Object.defineProperty(window, '__CMDR_COPY_PLATFORM__', {{ value: '{copy}' }});"))
        .unwrap_or_default();
    format!(
        "Object.defineProperty(window, '__CMDR_PLATFORM__', {{ value: '{platform}' }});\
         Object.defineProperty(window, '__CMDR_CAPABILITIES__', {{ value: Object.freeze({capabilities}) }});\
         {copy}\
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

    #[test]
    fn a_mocked_desktop_hands_its_copy_platform_only_when_mocked() {
        use super::super::LinuxDesktop;
        use super::super::conventions::{Desktop, conventions_on};
        let gnome = Desktop::Linux(LinuxDesktop::Gnome);
        let mocked = script_for(
            "macos",
            &capabilities(),
            &conventions_on(gnome),
            Some(gnome.copy_platform()),
        );
        assert!(mocked.contains("Object.defineProperty(window, '__CMDR_COPY_PLATFORM__', { value: 'linux' });"));
        assert!(mocked.starts_with("Object.defineProperty(window, '__CMDR_PLATFORM__', { value: 'macos' });"));
        assert!(!init_script().contains("__CMDR_COPY_PLATFORM__") || std::env::var("CMDR_MOCK_CONVENTIONS").is_ok());
    }
}
