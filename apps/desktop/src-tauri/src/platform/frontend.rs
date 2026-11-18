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
/// the platform a mocked row's copy reads (`None` when nothing is mocked). An E2E build may also
/// move `contextMenus` alone ([`with_e2e_context_menus`]).
fn frontend_conventions() -> (Option<&'static str>, PlatformConventions) {
    #[cfg(debug_assertions)]
    if let Some((desktop, mocked)) = conventions::mock_conventions() {
        log::info!(target: "platform", "CMDR_MOCK_CONVENTIONS: the frontend follows another desktop's conventions and copy");
        return (Some(desktop.copy_platform()), with_e2e_context_menus(mocked));
    }
    (None, with_e2e_context_menus(conventions()))
}

/// The env var an E2E run sets to `inPage` so the in-page right-click menus run on a lane whose OS
/// keeps native ones (Linux): `in-page-context-menus.spec.ts`, on the Linux lane's second launch.
#[cfg(any(feature = "playwright-e2e", test))]
const E2E_CONTEXT_MENUS_ENV: &str = "CMDR_E2E_CONTEXT_MENUS";

/// `conventions` with `contextMenus` from [`E2E_CONTEXT_MENUS_ENV`], in a `playwright-e2e` build
/// only: every other convention, the copy, and the keymaps stay this OS's. ❗ Compiled out of every
/// other build (release builds compile default features, which don't hold `playwright-e2e`), so
/// the variable means nothing to a shipped app; `the_e2e_context_menus_override_is_absent_here`
/// pins that.
fn with_e2e_context_menus(conventions: PlatformConventions) -> PlatformConventions {
    #[cfg(feature = "playwright-e2e")]
    if let Some(context_menus) = e2e_context_menus(std::env::var(E2E_CONTEXT_MENUS_ENV).ok().as_deref()) {
        log::info!(target: "platform", "{E2E_CONTEXT_MENUS_ENV}: the right-click menus follow the E2E run's pick");
        return PlatformConventions {
            context_menus,
            ..conventions
        };
    }
    conventions
}

/// The value of [`E2E_CONTEXT_MENUS_ENV`] as a convention: `inPage` or `native`, anything else none.
#[cfg(any(feature = "playwright-e2e", test))]
fn e2e_context_menus(value: Option<&str>) -> Option<conventions::ContextMenus> {
    match value {
        Some("inPage") => Some(conventions::ContextMenus::InPage),
        Some("native") => Some(conventions::ContextMenus::Native),
        _ => None,
    }
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
    fn reads_the_e2e_context_menus_value_strictly() {
        use super::conventions::ContextMenus;
        assert_eq!(e2e_context_menus(Some("inPage")), Some(ContextMenus::InPage));
        assert_eq!(e2e_context_menus(Some("native")), Some(ContextMenus::Native));
        assert_eq!(e2e_context_menus(Some("in-page")), None);
        assert_eq!(e2e_context_menus(Some("")), None);
        assert_eq!(e2e_context_menus(None), None);
    }

    /// A build without `playwright-e2e` (every release build) never reads the variable: an app
    /// launched with it answers this OS's own conventions.
    #[cfg(not(feature = "playwright-e2e"))]
    #[test]
    fn the_e2e_context_menus_override_is_absent_here() {
        let own = conventions();
        let flipped = match own.context_menus {
            conventions::ContextMenus::Native => "inPage",
            conventions::ContextMenus::InPage => "native",
        };
        // SAFETY: a test-local env var nothing else in this test binary reads or writes.
        unsafe { std::env::set_var(E2E_CONTEXT_MENUS_ENV, flipped) };
        let answered = with_e2e_context_menus(conventions());
        // SAFETY: as above.
        unsafe { std::env::remove_var(E2E_CONTEXT_MENUS_ENV) };
        assert_eq!(answered.context_menus, own.context_menus);
    }

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
