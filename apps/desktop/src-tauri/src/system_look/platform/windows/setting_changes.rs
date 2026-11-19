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
//! A signal re-reads all of it and compares with the last answer, rather than filtering on
//! `wParam` / `lParam`: a sender may pass 0, and the signals overlap (one accent change brings
//! `ColorValuesChanged` and a broadcast). A burst becomes ONE re-read: the first signal arms a
//! one-shot timer ([`SETTLE_MS`]), the rest ride on it, and the timer is killed when it fires, so
//! nothing ticks between bursts. That also lets a broadcaster's `SendMessage` return at once
//! instead of waiting on the read. An unchanged answer emits nothing. Other modules hear
//! `WM_SETTINGCHANGE` through [`on_setting_change`] (`intl/live_locale.rs` follows the Regional
//! format with it).

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use log::{debug, warn};
use tauri::AppHandle;
use tauri_specta::Event as _;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, MSG, PostMessageW, RegisterClassW,
    SetTimer, WM_APP, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WM_THEMECHANGED, WM_TIMER, WNDCLASSW,
};

use crate::ignore_poison::IgnorePoison;
use crate::system_events::{AccentColorChanged, DisplayAccessibility, DisplayAccessibilityChanged, SystemAccent};

/// Posted to the settings window by the `UISettings` event handlers.
const WM_UI_SETTINGS_CHANGED: u32 = WM_APP + 1;

/// How long a burst of signals gets to settle before its one re-read. Settings sends a change as
/// several broadcasts plus a `UISettings` event, milliseconds apart; this swallows them while the
/// UI still follows the user's click.
const SETTLE_MS: u32 = 100;
/// The settle timer's id, the settings window's only timer.
const SETTLE_TIMER: usize = 1;

/// The app the window procedure emits to, set once by [`observe`].
static APP: OnceLock<AppHandle> = OnceLock::new();
/// The look last sent (or read at start), so an unrelated signal emits nothing.
static LAST: Mutex<Option<Look>> = Mutex::new(None);
/// The settings window, as an integer so the `UISettings` handlers can post to it from any
/// thread. 0 until it exists.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Who else hears `WM_SETTINGCHANGE`, called on the settings window's thread.
static LISTENERS: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(Vec::new());
/// Whether the settle timer is armed for the current burst.
static SETTLING: Burst = Burst::new();

/// Collapses a burst of signals into one re-read: only the signal that opens a burst arms the
/// timer.
struct Burst {
    armed: AtomicBool,
}

impl Burst {
    const fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
        }
    }

    /// `true` for the signal that opens a burst (arm the timer), `false` for one it already covers.
    fn open(&self) -> bool {
        !self.armed.swap(true, Ordering::SeqCst)
    }

    /// The timer fired. Disarmed BEFORE the re-read, so a signal landing during it opens a fresh
    /// burst instead of being swallowed by this one.
    fn close(&self) {
        self.armed.store(false, Ordering::SeqCst);
    }
}

/// Everything this arm sends the frontend, compared as a whole on every signal.
#[derive(Debug, Clone, PartialEq)]
struct Look {
    accent: SystemAccent,
    display: DisplayAccessibility,
}

impl Look {
    fn read() -> Self {
        let (accent, display) = super::read_look();
        Self { accent, display }
    }
}

/// Runs `listener` on every `WM_SETTINGCHANGE` broadcast, on the settings window's thread. Keep it
/// quick (hand real work to another thread) and ❌ never panic: it runs inside the window
/// procedure. Works before or after [`observe`] starts the window, and from inside a listener.
pub(crate) fn on_setting_change(listener: Box<dyn Fn() + Send + Sync>) {
    LISTENERS.lock_ignore_poison().push(Arc::from(listener));
}

/// Calls every listener from a snapshot taken under the lock and run outside it, so a listener
/// that registers another (or takes any path back into [`on_setting_change`]) can't deadlock the
/// settings thread. One registered mid-call hears the next broadcast.
fn notify_listeners() {
    let listeners = LISTENERS.lock_ignore_poison().clone();
    for listener in &listeners {
        listener();
    }
}

/// Starts the settings window's thread. Call once, from the setup hook. Returns at once: the
/// baseline read happens on that thread, so its WinRT activation never holds up startup. The
/// frontend asks for its first look itself (`get_accent_color`, `get_display_accessibility`);
/// the baseline is only what later signals compare against, and it's read before the window
/// exists, so no signal can beat it.
pub(super) fn observe(app: AppHandle) {
    if APP.set(app).is_err() {
        warn!(target: "system_look", "Already following Windows setting changes, not starting a second watcher");
        return;
    }
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

/// Reads the baseline look, creates the hidden window, subscribes to `UISettings`, and pumps
/// messages until the process ends.
fn run_settings_window() {
    let started = std::time::Instant::now();
    let initial = Look::read();
    debug!(target: "system_look", "Windows look, read in {:?}: {initial:?}", started.elapsed());
    *LAST.lock_ignore_poison() = Some(initial);
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
            on_look_signal(window);
            notify_listeners();
        }
        WM_THEMECHANGED | WM_SYSCOLORCHANGE | WM_UI_SETTINGS_CHANGED => on_look_signal(window),
        WM_TIMER if wparam == SETTLE_TIMER => {
            // SAFETY: `window` is this procedure's own window, on the thread that owns it, and
            // `SETTLE_TIMER` is the timer `on_look_signal` armed on it.
            unsafe { KillTimer(window, SETTLE_TIMER) };
            SETTLING.close();
            reread_look();
            return 0;
        }
        _ => {}
    }
    // SAFETY: hands this window's own message, unchanged, to the default handler.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// One look signal: opens a burst (arms the settle timer) or joins the open one.
fn on_look_signal(window: HWND) {
    if !SETTLING.open() {
        return;
    }
    // SAFETY: `window` is the settings window, on the thread that created it, and no timer
    // procedure is passed, so the timer arrives as `WM_TIMER` in `window_proc`.
    if unsafe { SetTimer(window, SETTLE_TIMER, SETTLE_MS, None) } == 0 {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        let code = unsafe { GetLastError() };
        warn!(target: "system_look", "Couldn't arm the settle timer (error {code}), re-reading now");
        SETTLING.close();
        reread_look();
    }
}

/// Re-reads the look and emits each part that moved.
fn reread_look() {
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
    fn a_burst_arms_the_timer_once_and_a_fired_timer_opens_the_next() {
        let burst = Burst::new();
        assert!(burst.open(), "the first signal arms the timer");
        assert!(!burst.open(), "the rest of the burst rides on it");
        assert!(!burst.open());
        burst.close();
        assert!(burst.open(), "a signal after the re-read starts a new burst");
    }

    /// Regression: the window procedure once held the lock while calling listeners, so a
    /// listener that registered another deadlocked the settings thread.
    #[test]
    fn a_listener_may_register_another_listener() {
        use std::sync::atomic::AtomicUsize;

        static CALLS: AtomicUsize = AtomicUsize::new(0);
        on_setting_change(Box::new(|| {
            if CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
                on_setting_change(Box::new(|| {
                    CALLS.fetch_add(1, Ordering::SeqCst);
                }));
            }
        }));
        notify_listeners();
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            1,
            "the new listener waits for the next broadcast"
        );
        notify_listeners();
        assert_eq!(CALLS.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn with_nothing_sent_yet_everything_is_owed() {
        let blue = look("#0078d4", true);
        let sent = changes(None, &blue);
        assert!(sent.accent.is_some());
        assert!(sent.display.is_some());
    }
}
