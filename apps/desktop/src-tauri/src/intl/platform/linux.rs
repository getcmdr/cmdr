//! Linux: no preference list, only the session's locale environment, which WebKit reads too.

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

/// The session's locale environment (`LC_ALL` / `LC_MESSAGES` / `LANG`).
pub(crate) fn os_ui_locale() -> String {
    super::locale_env::posix_ui_locale()
}
