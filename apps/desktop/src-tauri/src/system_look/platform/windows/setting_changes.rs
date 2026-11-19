//! Follows every Windows setting the look reads, live, and emits `accent-color-changed` and
//! `display-accessibility-changed` when one moved. Four signals feed one re-read:
//!
//! - `WM_SETTINGCHANGE`: the broadcast Windows sends every top-level window when a system setting
//!   moves (`SystemParametersInfoW` with `SPIF_SENDCHANGE`, which Settings uses): Underline access
//!   keys, a contrast theme, the Regional format (`"intl"`), and `"ImmersiveColorSet"` for the
//!   accent and light/dark.
//! - `WM_THEMECHANGED` and `WM_SYSCOLORCHANGE`: a contrast theme switching on, off, or to another.
//! - `UISettings`' `ColorValuesChanged` and `AdvancedEffectsEnabledChanged`, the documented
//!   signals for the accent and Transparency effects. They fire on a thread pool thread, so the
//!   handler posts [`WM_UI_SETTINGS_CHANGED`] here and the re-read stays on this one thread.
//!
//! The broadcasts need a window of our own. A hidden top-level one on a dedicated thread with its
//! own message loop, ❌ not a message-only window (`HWND_MESSAGE`): those never receive
//! broadcasts. Its own thread keeps it apart from tao's windows and their loop.
//!
//! Every signal re-reads all of it and compares with the last answer, rather than filtering on
//! `wParam` / `lParam`: a sender may pass 0, the signals overlap (one accent change brings
//! `ColorValuesChanged` and a broadcast), and one cheap read per signal is nothing. An unchanged
//! answer emits nothing. Other modules hear `WM_SETTINGCHANGE` through [`on_setting_change`]
//! (`intl/live_locale.rs` follows the Regional format with it).

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use log::{debug, warn};
use tauri::AppHandle;
use tauri_specta::Event as _;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostMessageW, RegisterClassW, WM_APP,
    WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WM_THEMECHANGED, WNDCLASSW,
};

use crate::ignore_poison::IgnorePoison;
use crate::system_events::{AccentColorChanged, DisplayAccessibility, DisplayAccessibilityChanged, SystemAccent};

/// Posted to the settings window by the `UISettings` event handlers.
const WM_UI_SETTINGS_CHANGED: u32 = WM_APP + 1;

/// The app the window procedure emits to, set once by [`observe`].
static APP: OnceLock<AppHandle> = OnceLock::new();
/// The look last sent (or read at start), so an unrelated signal emits nothing.
static LAST: Mutex<Option<Look>> = Mutex::new(None);
/// The settings window, as an integer so the `UISettings` handlers can post to it from any
/// thread. 0 until it exists.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Who else hears `WM_SETTINGCHANGE`, called on the settings window's thread.
static LISTENERS: Mutex<Vec<Box<dyn Fn() + Send>>> = Mutex::new(Vec::new());

/// Everything this arm sends the frontend, compared as a whole on every signal.
#[derive(Debug, Clone, PartialEq)]
struct Look {
    accent: SystemAccent,
    display: DisplayAccessibility,
}

impl Look {
    fn read() -> Self {
        Self {
            accent: super::read_accent(),
            display: super::read_display_accessibility(),
        }
    }
}

/// Runs `listener` on every `WM_SETTINGCHANGE` broadcast, on the settings window's thread. Keep it
/// quick (hand real work to another thread) and ❌ never panic: it runs inside the window
/// procedure. Works before or after [`observe`] starts the window.
pub(crate) fn on_setting_change(listener: Box<dyn Fn() + Send>) {
    LISTENERS.lock_ignore_poison().push(listener);
}

/// Starts the settings window's thread. Call once, from the setup hook.
pub(super) fn observe(app: AppHandle) {
    if APP.set(app).is_err() {
        warn!(target: "system_look", "Already following Windows setting changes, not starting a second watcher");
        return;
    }
    let initial = Look::read();
    debug!(target: "system_look", "Windows look: {initial:?}");
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

/// Creates the hidden window, subscribes to `UISettings`, and pumps messages until the process
/// ends.
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
    WINDOW.store(window as isize, Ordering::SeqCst);
    // Held for the loop's whole life: dropping it would end its event registrations.
    let _ui_settings = super::ui_settings::settings()
        .and_then(|settings| super::ui_settings::subscribe(&settings, post_ui_settings_changed).map(|()| settings))
        .inspect_err(|error| {
            warn!(target: "system_look", "Couldn't follow the accent and Transparency effects ({error}); the settings broadcasts still apply");
        })
        .ok();
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

/// The `UISettings` handlers' hop onto the settings window's thread. Called on a thread pool
/// thread; ❌ never panic here either.
fn post_ui_settings_changed() {
    let window = WINDOW.load(Ordering::SeqCst) as HWND;
    if window.is_null() {
        return;
    }
    // SAFETY: `window` is the settings window, which lives until the process ends (nothing
    // destroys it), and `PostMessageW` may be called from any thread. The message carries no
    // pointers.
    if unsafe { PostMessageW(window, WM_UI_SETTINGS_CHANGED, 0, 0) } == 0 {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        let code = unsafe { GetLastError() };
        warn!(target: "system_look", "Couldn't pass on a UISettings change (error {code})");
    }
}

/// The settings window's procedure: a look signal re-reads the look, a `WM_SETTINGCHANGE` also
/// tells the listeners, and every message then goes on to `DefWindowProcW`. ❌ Nothing here may
/// panic: it runs across the FFI boundary.
unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_SETTINGCHANGE => {
            on_look_signal();
            for listener in LISTENERS.lock_ignore_poison().iter() {
                listener();
            }
        }
        WM_THEMECHANGED | WM_SYSCOLORCHANGE | WM_UI_SETTINGS_CHANGED => on_look_signal(),
        _ => {}
    }
    // SAFETY: hands this window's own message, unchanged, to the default handler.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn on_look_signal() {
    let next = Look::read();
    let previous = LAST.lock_ignore_poison().replace(next.clone());
    let changes = changes(previous.as_ref(), &next);
    let Some(app) = APP.get() else {
        return;
    };
    if let Some(event) = changes.accent {
        debug!(target: "system_look", "Accent changed: {:?}", event.accent);
        if let Err(error) = event.emit(app) {
            warn!(target: "system_look", "Couldn't send the accent color change ({error})");
        }
    }
    if let Some(event) = changes.display {
        debug!(target: "system_look", "Display accessibility changed: {:?}", event.options);
        if let Err(error) = event.emit(app) {
            warn!(target: "system_look", "Couldn't send the display accessibility change ({error})");
        }
    }
}

/// The events one re-read owes the frontend.
#[derive(Debug, Default)]
struct Changes {
    accent: Option<AccentColorChanged>,
    display: Option<DisplayAccessibilityChanged>,
}

/// Each part of `next` that differs from what was last sent, as its event. With nothing sent yet,
/// both are owed.
fn changes(previous: Option<&Look>, next: &Look) -> Changes {
    Changes {
        accent: (previous.map(|look| &look.accent) != Some(&next.accent)).then(|| AccentColorChanged {
            accent: next.accent.clone(),
        }),
        display: (previous.map(|look| look.display) != Some(next.display))
            .then_some(DisplayAccessibilityChanged { options: next.display }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_events::AccentFill;

    fn look(accent: &str, underline_access_keys: bool) -> Look {
        Look {
            accent: SystemAccent {
                hex: accent.to_owned(),
                fill: Some(AccentFill {
                    light: "#005a9e".to_owned(),
                    dark: "#99ebff".to_owned(),
                }),
            },
            display: DisplayAccessibility {
                underline_access_keys,
                ..DisplayAccessibility::default()
            },
        }
    }

    #[test]
    fn a_signal_that_moved_nothing_sends_nothing() {
        let blue = look("#0078d4", false);
        let sent = changes(Some(&blue), &blue);
        assert!(sent.accent.is_none());
        assert!(sent.display.is_none());
    }

    #[test]
    fn only_the_part_that_moved_is_sent() {
        let blue = look("#0078d4", false);
        let red = look("#e81123", false);
        let sent = changes(Some(&blue), &red);
        assert_eq!(sent.accent.map(|event| event.accent), Some(red.accent.clone()));
        assert!(sent.display.is_none());

        let underlined = look("#0078d4", true);
        let sent = changes(Some(&blue), &underlined);
        assert!(sent.accent.is_none());
        assert_eq!(sent.display.map(|event| event.options), Some(underlined.display));
    }

    #[test]
    fn a_shade_alone_moving_resends_the_accent() {
        let blue = look("#0078d4", false);
        let mut reshaded = blue.clone();
        if let Some(fill) = reshaded.accent.fill.as_mut() {
            fill.dark = "#76b9ed".to_owned();
        }
        assert!(changes(Some(&blue), &reshaded).accent.is_some());
    }

    #[test]
    fn with_nothing_sent_yet_everything_is_owed() {
        let blue = look("#0078d4", true);
        let sent = changes(None, &blue);
        assert!(sent.accent.is_some());
        assert!(sent.display.is_some());
    }
}
