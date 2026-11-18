//! Windows' "Underline access keys" (Settings › Accessibility › Keyboard): with it on, Win32
//! draws access-key underlines all the time, not only once the keyboard is in use. Cmdr's in-page
//! menus and dialogs follow it (`apps/desktop/src/lib/display-accessibility.ts`).
//!
//! `SPI_GETKEYBOARDCUES` and `SPI_GETMENUUNDERLINES` are the same action (`0x100A`), so one read
//! covers menus and dialogs alike. The setting is per user and changes without a restart, so it's
//! read fresh on every ask and on every `WM_SETTINGCHANGE`.

use log::warn;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETKEYBOARDCUES, SystemParametersInfoW};
use windows_sys::core::BOOL;

use crate::system_events::DisplayAccessibility;

/// Whether Underline access keys is on, or `false` (Windows' default) when the read fails.
pub(super) fn underline_access_keys() -> bool {
    let mut on: BOOL = 0;
    // SAFETY: `SPI_GETKEYBOARDCUES` writes one `BOOL` through `pvParam`, which points at `on`, a
    // live `BOOL` on this stack for the whole call; `uiParam` is unused (0), and `fWinIni` 0 sends
    // no broadcast.
    let ok = unsafe { SystemParametersInfoW(SPI_GETKEYBOARDCUES, 0, (&raw mut on).cast(), 0) };
    if ok == 0 {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        let code = unsafe { GetLastError() };
        warn!(target: "system_look", "Couldn't read Underline access keys (error {code}), treating it as off");
        return false;
    }
    on != 0
}

/// The display options as Windows answers them: Underline access keys, every macOS-only option off.
pub(super) fn display_accessibility_from(underline_access_keys: bool) -> DisplayAccessibility {
    DisplayAccessibility {
        underline_access_keys,
        ..DisplayAccessibility::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underline_access_keys_maps_to_its_own_field_alone() {
        let on = display_accessibility_from(true);
        assert!(on.underline_access_keys);
        assert_eq!(
            on,
            DisplayAccessibility {
                underline_access_keys: true,
                ..DisplayAccessibility::default()
            }
        );
        assert_eq!(display_accessibility_from(false), DisplayAccessibility::default());
    }
}
