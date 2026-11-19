//! Windows: the user's ordered preferred UI languages (`GetUserPreferredUILanguages`), resolved
//! against the catalogs we ship, and the Regional format's locale name
//! (`GetUserDefaultLocaleName`) for the formatting half. Both halves answer the same, so the menu
//! and the window always agree, as on macOS.

use std::sync::Arc;

use log::warn;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Globalization::{GetUserDefaultLocaleName, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

/// `LOCALE_NAME_MAX_LENGTH` (`winnls.h`): the longest locale name, terminator included. Spelled
/// here rather than enabling `windows-sys`'s whole `SystemServices` module for one constant.
const LOCALE_NAME_MAX_LENGTH: usize = 85;

use super::super::SHIPPED_LOCALES;
use super::super::format_locale::format_locale_from_windows_name;
use super::super::native_strings::NativeOs;
use super::super::resolve_ui_locale;

/// Native strings read their `windows` text here. Never Linux's: `cmdr-menu`'s `Platform`
/// folds Windows into Linux, which is why text doesn't ask it.
pub(crate) fn native_text_os() -> NativeOs {
    NativeOs::Windows
}

/// The resolved preference list. Windows always answers, falling back to `en`.
pub(crate) fn webview_ui_locale() -> Option<String> {
    Some(os_ui_locale())
}

/// Read fresh from the OS every time, so a live change shows (`live_locale.rs`).
pub(crate) fn os_ui_locale() -> String {
    resolve_ui_locale(&preferred_ui_languages(), SHIPPED_LOCALES).unwrap_or_else(|| "en".to_string())
}

/// The user's preferred UI languages, in the user's order (`hu-HU`, `en-US`): the display
/// language first, then its fallbacks. Empty when the read fails, which resolves to `en`.
fn preferred_ui_languages() -> Vec<String> {
    let mut count: u32 = 0;
    let mut length: u32 = 0;
    // SAFETY: the sizing call: a null buffer with a zero length asks only for the length needed,
    // written through `length`; `count` receives the language count. Both are live `u32`s here.
    let sized = unsafe {
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &raw mut count, std::ptr::null_mut(), &raw mut length)
    };
    if sized == 0 || length == 0 {
        log_last_error("size the preferred UI languages");
        return Vec::new();
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: `buffer` holds exactly `length` UTF-16 units, the size the call above asked for, and
    // stays alive and unmoved for the call; `length` tells the API that size.
    let read =
        unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &raw mut count, buffer.as_mut_ptr(), &raw mut length) };
    if read == 0 {
        log_last_error("read the preferred UI languages");
        return Vec::new();
    }
    split_multi_string(&buffer)
}

/// Calls `on_signal` on every `WM_SETTINGCHANGE` broadcast, for the whole session.
///
/// Windows broadcasts it (with `"intl"`) when the Regional format moves, and the system look's
/// settings window already hears every broadcast, so this rides on it rather than making a second
/// window. Unfiltered: `live_locale.rs`'s watcher collapses a burst into one cheap re-read, and an
/// unchanged pair announces nothing. A display-language change usually takes a sign-out to apply;
/// the re-read picks it up whenever `GetUserPreferredUILanguages` reports it.
pub(crate) fn observe_locale_signals(on_signal: Arc<dyn Fn() + Send + Sync>) {
    crate::system_look::platform::on_setting_change(Box::new(move || on_signal()));
}

/// Nothing cached per system language.
pub(crate) fn forget_system_language_caches() {}

/// The tag the user formats in, from Windows' Regional format (Settings › Time & language ›
/// Region), which WebView2 doesn't follow: its `Intl` default is the DISPLAY language, so a US
/// English Windows set to Swedish formats would still write `08/19/2026`.
pub(crate) fn format_locale() -> Option<String> {
    format_locale_from_windows_name(&user_format_locale_name()?)
}

/// The Regional format's locale name (`en-SE`, `de-DE_phoneb`), or `None` when the read fails.
fn user_format_locale_name() -> Option<String> {
    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: `buffer` is a live array of `LOCALE_NAME_MAX_LENGTH` units, the size passed (85
    // fits an `i32`), which is the documented maximum a locale name takes, terminator included.
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), LOCALE_NAME_MAX_LENGTH as i32) };
    if written <= 0 {
        log_last_error("read the Regional format");
        return None;
    }
    split_multi_string(&buffer).into_iter().next()
}

fn log_last_error(what: &str) {
    // SAFETY: reads this thread's last-error value, right after the call that set it.
    let code = unsafe { GetLastError() };
    warn!(target: "intl", "Couldn't {what} (error {code}), falling back to the defaults");
}

/// Splits a Win32 multi-string (`a\0b\0\0`) into its entries, stopping at the first empty one.
fn split_multi_string(units: &[u16]) -> Vec<String> {
    units
        .split(|&unit| unit == 0)
        .take_while(|entry| !entry.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn a_multi_string_splits_in_order_and_stops_at_the_double_terminator() {
        assert_eq!(
            split_multi_string(&wide("hu-HU\0en-US\0\0garbage\0")),
            vec!["hu-HU".to_string(), "en-US".to_string()]
        );
        assert_eq!(split_multi_string(&wide("en-SE\0\0\0\0")), vec!["en-SE".to_string()]);
        assert!(split_multi_string(&wide("\0\0")).is_empty());
        assert!(split_multi_string(&[]).is_empty());
    }

    /// The real machine: whatever it's set to, the list reads, and its answer is a catalog we ship.
    #[test]
    fn this_machines_languages_resolve_to_a_shipped_catalog() {
        let languages = preferred_ui_languages();
        assert!(!languages.is_empty(), "Windows always has a display language");
        let resolved = os_ui_locale();
        assert!(
            super::super::super::is_shipped_locale(&resolved),
            "{resolved} isn't shipped"
        );
        log::info!(target: "intl", "This machine: languages {languages:?} -> {resolved}, formats {:?} ({:?})", user_format_locale_name(), format_locale());
    }
}
