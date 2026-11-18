//! Follows `WM_SETTINGCHANGE`, the broadcast Windows sends every top-level window when a system
//! setting moves (`SystemParametersInfoW` with `SPIF_SENDCHANGE`, which Settings uses), and emits
//! `display-accessibility-changed` when the options this arm reads changed.
//!
//! The broadcast needs a window of our own. A hidden top-level one on a dedicated thread with its
//! own message loop, ❌ not a message-only window (`HWND_MESSAGE`): those never receive
//! broadcasts. Its own thread keeps it apart from tao's windows and their loop.
//!
//! Every `WM_SETTINGCHANGE` re-reads all options and compares with the last answer, rather than
//! filtering on `wParam`: a sender may pass 0, and one cheap read per broadcast is nothing. Task 40
//! adds its readers (high contrast, transparency) to the same re-read.

use std::sync::{Mutex, OnceLock};

use log::{debug, warn};
use tauri::AppHandle;
use tauri_specta::Event as _;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, RegisterClassW, WM_SETTINGCHANGE, WNDCLASSW,
};

use crate::ignore_poison::IgnorePoison;
use crate::system_events::{DisplayAccessibility, DisplayAccessibilityChanged};

/// The app the window procedure emits to, set once by [`observe`].
static APP: OnceLock<AppHandle> = OnceLock::new();
/// The options last sent (or read at start), so an unrelated broadcast emits nothing.
static LAST: Mutex<Option<DisplayAccessibility>> = Mutex::new(None);

/// Starts the settings window's thread. Call once, from the setup hook.
pub(super) fn observe(app: AppHandle) {
    if APP.set(app).is_err() {
        warn!(target: "system_look", "Already following Windows setting changes, not starting a second watcher");
        return;
    }
    let initial = super::read_display_accessibility();
    debug!(target: "system_look", "Display accessibility: {initial:?}");
    *LAST.lock_ignore_poison() = Some(initial);
    if let Err(error) = std::thread::Builder::new()
        .name("system-look-settings".to_owned())
        .spawn(run_settings_window)
    {
        warn!(target: "system_look", "Couldn't start following Windows setting changes ({error}); they'll apply on restart");
    }
}

/// UTF-16, NUL-terminated, for a `PCWSTR`.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Creates the hidden window and pumps its messages until the process ends.
fn run_settings_window() {
    let class_name = wide("CmdrSystemLookSettings");
    // SAFETY: a null name asks for the module that started this process (cmdr.exe), loaded for
    // the process's whole life; the handle needs no release.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..WNDCLASSW::default()
    };
    // SAFETY: `class` is a fully initialized `WNDCLASSW` (the rest zeroed, which means "none"),
    // and its name points into `class_name`, which lives until this function returns, after every
    // use of the class.
    if unsafe { RegisterClassW(&raw const class) } == 0 {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        let code = unsafe { GetLastError() };
        warn!(target: "system_look", "Couldn't register the settings window class (error {code}); setting changes apply on restart");
        return;
    }
    // SAFETY: the class was registered just above, in this module, under this name. No parent, no
    // `WS_VISIBLE`: a hidden top-level window, which receives broadcasts. The window name may be
    // null, and no creation parameter is passed.
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if window.is_null() {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        let code = unsafe { GetLastError() };
        warn!(target: "system_look", "Couldn't create the settings window (error {code}); setting changes apply on restart");
        return;
    }
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is a live `MSG` this loop owns; a null window takes every message
        // posted or sent to this thread's windows.
        let got = unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) };
        // 0 is `WM_QUIT`, -1 a failure: either way the loop is over.
        if got == 0 || got == -1 {
            break;
        }
        // SAFETY: `message` was filled by `GetMessageW` just above.
        unsafe { DispatchMessageW(&raw const message) };
    }
}

/// The settings window's procedure: a `WM_SETTINGCHANGE` re-reads the options, and every message
/// then goes on to `DefWindowProcW`. ❌ Nothing here may panic: it runs across the FFI boundary.
unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_SETTINGCHANGE {
        on_setting_change();
    }
    // SAFETY: hands this window's own message, unchanged, to the default handler.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn on_setting_change() {
    let next = super::read_display_accessibility();
    let previous = LAST.lock_ignore_poison().replace(next);
    let Some(changed) = changed(previous, next) else {
        return;
    };
    debug!(target: "system_look", "Display accessibility changed: {next:?}");
    if let Some(app) = APP.get()
        && let Err(error) = changed.emit(app)
    {
        warn!(target: "system_look", "Couldn't send the display accessibility change ({error})");
    }
}

/// The event to send when `next` differs from what was last sent, else `None`.
fn changed(previous: Option<DisplayAccessibility>, next: DisplayAccessibility) -> Option<DisplayAccessibilityChanged> {
    (previous != Some(next)).then_some(DisplayAccessibilityChanged { options: next })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broadcast_that_moved_nothing_sends_nothing() {
        let off = DisplayAccessibility::default();
        let on = DisplayAccessibility {
            underline_access_keys: true,
            ..off
        };
        assert!(changed(Some(off), off).is_none());
        assert!(changed(Some(on), on).is_none());
        assert_eq!(changed(Some(off), on).map(|event| event.options), Some(on));
        assert_eq!(changed(Some(on), off).map(|event| event.options), Some(off));
        assert_eq!(changed(None, off).map(|event| event.options), Some(off));
    }
}
