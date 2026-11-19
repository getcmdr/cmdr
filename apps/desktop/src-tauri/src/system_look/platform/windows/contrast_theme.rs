//! Whether a Windows contrast theme is on (Settings › Accessibility › Contrast themes; "High
//! contrast" on Windows 10), read with `SPI_GETHIGHCONTRAST`.
//!
//! The colors themselves need no reader: WebView2 turns a contrast theme into
//! `forced-colors: active`, which `app-controls-windows-contrast.css` and `app-menu.css` style
//! from, live. This flag is for what CSS can't key on: it sets `increase_contrast` (the same
//! stronger borders and focus rings macOS's Increase contrast gets) and drops translucency, as
//! WinUI does under a contrast theme. A theme switch broadcasts `WM_SETTINGCHANGE`
//! (`SPI_SETHIGHCONTRAST`) and `WM_SYSCOLORCHANGE`, which the settings window hears.

use log::warn;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW};

/// Whether a contrast theme is on, or `false` (Windows' default) when the read fails.
pub(super) fn contrast_theme_on() -> bool {
    let size = size_of::<HIGHCONTRASTW>() as u32;
    let mut settings = HIGHCONTRASTW {
        cbSize: size,
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    // SAFETY: `SPI_GETHIGHCONTRAST` fills the `HIGHCONTRASTW` that `pvParam` points at, a live
    // struct on this stack whose `cbSize` is set, as the call requires, with `uiParam` the same
    // size; `fWinIni` 0 sends no broadcast. The scheme name it hands back points into system
    // memory we never read.
    let ok = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, size, (&raw mut settings).cast(), 0) };
    if ok == 0 {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        let code = unsafe { GetLastError() };
        warn!(target: "system_look", "Couldn't read the contrast theme (error {code}), treating it as off");
        return false;
    }
    is_on(settings.dwFlags)
}

/// The on bit of `HIGHCONTRASTW::dwFlags`; the other bits say what the hotkey and the settings
/// dialog may do.
fn is_on(flags: u32) -> bool {
    flags & HCF_HIGHCONTRASTON != 0
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::Accessibility::{HCF_AVAILABLE, HCF_HOTKEYACTIVE};

    use super::*;

    #[test]
    fn only_the_on_bit_turns_a_theme_on() {
        assert!(!is_on(0));
        assert!(!is_on(HCF_AVAILABLE | HCF_HOTKEYACTIVE));
        assert!(is_on(HCF_HIGHCONTRASTON));
        assert!(is_on(HCF_HIGHCONTRASTON | HCF_AVAILABLE));
    }
}
