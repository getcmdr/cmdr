//! The `SC_KEYMENU` guard: Alt and F10 reach Cmdr's in-page menu bar and never start Windows' own
//! menu loop (task 63 step 6 of `docs/specs/windows-port-plan.md`, D5).
//!
//! A Cmdr window has no Win32 menu: the page draws its bar (`#lib/ui/MenuBar.svelte`). WebView2
//! hands a key the page didn't handle to the host window, so a lone Alt's release and an F10 reach
//! `DefWindowProc` as `WM_SYSCOMMAND` `SC_KEYMENU` with `lParam` 0, and an Alt+letter no title took
//! as `SC_KEYMENU` with the letter. With no menu bar, Windows then starts its menu loop on the
//! invisible system menu (the next arrow or letter goes to it, not the page) or beeps for the
//! letter (measured on Windows 10 22H2 with the menu spike, 2026-11-18,
//! `docs/notes/windows-port/menu-spike.md`). The page has already acted on the key (the bar's
//! keyup handler takes a lone Alt), so the window answers every `SC_KEYMENU` itself, except
//! Alt+Space (`lParam` 0x20), which opens the window's system menu (Restore, Move, Size, Close).
//!
//! Installed on every Cmdr window from `prepare_webview`: a window without a bar (Settings, the
//! queue) gains nothing from the menu loop either, and the page's own Alt+letter access keys stay
//! its own.

use tauri::{Manager as _, Webview, Wry};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{SC_KEYMENU, WM_NCDESTROY, WM_SYSCOMMAND};

/// The subclass's id on the window (any constant unique to us; `SetWindowSubclass` with the same
/// id and procedure again only replaces its data, so a second webview in one window is harmless).
const SUBCLASS_ID: usize = 0xC3D7_0063;

/// `SC_KEYMENU`'s `lParam` for Alt+Space: the window's system menu, which stays Windows'.
const ALT_SPACE: LPARAM = 0x20;

/// Whether the window answers this `WM_SYSCOMMAND` itself rather than passing it to Windows.
/// The low four bits of `wParam` are Windows' own (`SC_*` values are multiples of 16).
pub(crate) fn swallows(msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    msg == WM_SYSCOMMAND && (wparam & 0xFFF0) == SC_KEYMENU as WPARAM && lparam != ALT_SPACE
}

/// Whether the guard takes itself off the window on this message: `WM_NCDESTROY`, the last one a
/// window gets, as comctl32 asks of every subclass (it doesn't remove one for us).
pub(crate) fn removes_itself_on(msg: u32) -> bool {
    msg == WM_NCDESTROY
}

/// Subclasses `webview`'s window with the guard. A failure is logged, never fatal: the window
/// works, only a lone Alt or F10 starts Windows' menu loop there.
pub(crate) fn install(webview: &Webview<Wry>) {
    let window = webview.window();
    let label = window.label().to_owned();
    // `SetWindowSubclass` has to run on the thread that owns the window: Tauri's main thread.
    let dispatched = webview.app_handle().run_on_main_thread(move || {
        let hwnd = match window.hwnd() {
            Ok(hwnd) => hwnd.0,
            Err(e) => {
                // allowed-log-private: a Tauri window label of our own
                log::warn!(target: "platform::keymenu", "no window handle for `{label}`, so Alt may start Windows' menu loop there: {e}");
                return;
            }
        };
        // SAFETY: `hwnd` is the live top-level window Tauri just handed over, and this runs on the
        // thread that owns it (the main thread), which `SetWindowSubclass` requires. `subclass` is
        // a `'static` `extern "system"` fn matching `SUBCLASSPROC`, and it reads no reference data
        // (0), so nothing it touches can dangle. It removes itself on `WM_NCDESTROY` (`subclass`).
        let installed = unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0) };
        if installed == 0 {
            // allowed-log-private: a Tauri window label of our own
            log::warn!(target: "platform::keymenu", "couldn't guard `{label}` against Windows' menu loop");
        }
    });
    if let Err(e) = dispatched {
        log::warn!(target: "platform::keymenu", "couldn't reach the main thread to guard a window: {e}");
    }
}

/// The window procedure in front of the window's own: answers a swallowed `SC_KEYMENU` with 0 (the
/// documented "processed" answer for `WM_SYSCOMMAND`), passes everything else on, and takes itself
/// off the window as it goes.
unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if swallows(msg, wparam, lparam) {
        return 0;
    }
    if removes_itself_on(msg) {
        // SAFETY: `hwnd` is the window this procedure was installed on with `SUBCLASS_ID`, and
        // comctl32 calls it on the thread that owns the window, which `RemoveWindowSubclass`
        // requires. The message still goes on to `DefSubclassProc` below, which may run after the
        // removal, as the documented `WM_NCDESTROY` pattern does.
        let removed = unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) };
        if removed == 0 {
            log::debug!(target: "platform::keymenu", "the menu-loop guard was already off a closing window");
        }
    }
    // SAFETY: comctl32 calls this procedure with the window's own message on its own thread, and
    // `DefSubclassProc` is the documented way to hand it to the next procedure in the chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SC_CLOSE, SC_MINIMIZE, WM_KEYDOWN};

    #[test]
    fn a_lone_alt_or_f10_is_the_pages() {
        assert!(swallows(WM_SYSCOMMAND, SC_KEYMENU as WPARAM, 0));
    }

    #[test]
    fn an_alt_letter_no_title_took_is_the_pages_too() {
        assert!(swallows(WM_SYSCOMMAND, SC_KEYMENU as WPARAM, LPARAM::from(b'f')));
    }

    #[test]
    fn windows_own_low_bits_dont_hide_a_keymenu() {
        assert!(swallows(WM_SYSCOMMAND, SC_KEYMENU as WPARAM | 0x2, 0));
    }

    #[test]
    fn alt_space_opens_the_system_menu() {
        assert!(!swallows(WM_SYSCOMMAND, SC_KEYMENU as WPARAM, ALT_SPACE));
    }

    #[test]
    fn the_guard_comes_off_with_the_windows_last_message() {
        assert!(removes_itself_on(WM_NCDESTROY));
        assert!(!removes_itself_on(WM_SYSCOMMAND));
    }

    #[test]
    fn every_other_system_command_passes() {
        assert!(!swallows(WM_SYSCOMMAND, SC_CLOSE as WPARAM, 0));
        assert!(!swallows(WM_SYSCOMMAND, SC_MINIMIZE as WPARAM, 0));
        assert!(!swallows(WM_KEYDOWN, SC_KEYMENU as WPARAM, 0));
    }
}
