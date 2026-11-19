//! Linux: no preference list, only the session's locale environment, which WebKit reads too.

use std::sync::Arc;

use super::super::native_strings::NativeOs;
use crate::platform::LinuxDesktop;

/// Native strings read this session's desktop's text here: the `linuxGnome` or `linuxKde` branch
/// on those desktops, else `other`. The desktop is the platform conventions' answer, the one
/// the webview gets, so the menu bar and the window speak the same desktop (the debug-only
/// `CMDR_MOCK_CONVENTIONS` moves the webview alone).
pub(crate) fn native_text_os() -> NativeOs {
    NativeOs::Linux(
        crate::platform::conventions()
            .linux_desktop
            .unwrap_or(LinuxDesktop::Other),
    )
}

/// `None`: WebKit reads the same session environment the desktop does, so its default is right.
pub(crate) fn webview_ui_locale() -> Option<String> {
    None
}

/// No signal to follow, so `on_signal` is dropped.
///
/// The desktop language and formats live in the session's environment (`LANG` / `LC_MESSAGES` /
/// `LC_NUMERIC`), which is fixed for the life of the process, and no portal or D-Bus name
/// broadcasts a change. A logged-in user changing either gets it at their next login, which is
/// also when Cmdr restarts. ❌ Don't go looking for a watcher here; there isn't one.
pub(crate) fn observe_locale_signals(_on_signal: Arc<dyn Fn() + Send + Sync>) {}

/// Nothing cached per system language.
pub(crate) fn forget_system_language_caches() {}

/// `None`: the desktop's formatting conventions come from the same session environment WebKit
/// already reads, so the webview's default is the right answer.
pub(crate) fn format_locale() -> Option<String> {
    None
}

/// The session's locale environment (`LC_ALL` / `LC_MESSAGES` / `LANG`).
pub(crate) fn os_ui_locale() -> String {
    super::locale_env::posix_ui_locale()
}
