//! macOS: the user's ordered `AppleLanguages` list, resolved against the catalogs we ship. Both
//! halves answer the same, so the menu bar and the webview always agree.

use std::sync::Arc;

use super::super::SHIPPED_LOCALES;
use super::super::format_locale::compose_format_locale;
use super::super::native_strings::NativeOs;
use super::super::resolve_ui_locale;

/// Native strings read their `macos` text here.
pub(crate) fn native_text_os() -> NativeOs {
    NativeOs::MacOs
}

/// The resolved preference list. macOS always answers, falling back to `en`.
pub(crate) fn webview_ui_locale() -> Option<String> {
    Some(os_ui_locale())
}

/// Read fresh from the OS preference list every time. Uncached on purpose: the read is a
/// `NSUserDefaults` lookup (~65 µs), and caching it is what would make a live language change
/// invisible.
pub(crate) fn os_ui_locale() -> String {
    let preferences = crate::system_strings::apple_languages();
    resolve_ui_locale(&preferences, SHIPPED_LOCALES).unwrap_or_else(|| "en".to_string())
}

/// Calls `on_signal` on every macOS language or region notification, for the whole session.
///
/// Two notification names, and they carry different halves of the answer.
/// `AppleLanguagePreferencesChangedNotification` is the distributed notification the System
/// Settings pane posts when the LANGUAGE order moves; it's undocumented in the same way
/// `system_look/platform/macos/text_size.rs`'s accessibility notification is, with the same
/// fallback story (if Apple stops posting it, the language still resolves correctly on the next
/// launch). `NSCurrentLocaleDidChangeNotification` is the documented signal and tracks
/// `AppleLocale`, which is where the REGION override lives, so it's what carries a region change
/// to the formatters. Overlap costs nothing: `live_locale.rs`'s watcher collapses a burst into one
/// re-read. The evidence for both claims, and why `defaults write` alone can't test this, is in
/// `DETAILS.md`. The registration has no main-thread requirement.
pub(crate) fn observe_locale_signals(on_signal: Arc<dyn Fn() + Send + Sync>) {
    use std::ptr::NonNull;

    use objc2_foundation::{
        NSCurrentLocaleDidChangeNotification, NSDistributedNotificationCenter, NSNotification, NSNotificationCenter,
        NSString,
    };

    let default_center = NSNotificationCenter::defaultCenter();
    let distributed_center = NSDistributedNotificationCenter::defaultCenter();
    let language_preferences_changed = NSString::from_str("AppleLanguagePreferencesChangedNotification");

    let for_default = Arc::clone(&on_signal);
    let default_block = block2::RcBlock::new(move |_notification: NonNull<NSNotification>| {
        for_default();
    });
    let distributed_block = block2::RcBlock::new(move |_notification: NonNull<NSNotification>| {
        on_signal();
    });

    // SAFETY: both names are valid notification names (one the Foundation
    // constant, one an `NSString` we own) and both centres are live singletons.
    // Each block is a live `RcBlock` with the expected
    // `(NonNull<NSNotification>) -> ()` signature. The centres retain the
    // observers for the lifetime of the app; we never remove them, because we
    // want updates for the whole session.
    unsafe {
        default_center.addObserverForName_object_queue_usingBlock(
            Some(NSCurrentLocaleDidChangeNotification),
            None,
            None,
            &default_block,
        );
        distributed_center.addObserverForName_object_queue_usingBlock(
            Some(&language_preferences_changed),
            None,
            None,
            &distributed_block,
        );
    }
}

/// The macOS pane labels we quote ("Full Disk Access", "Privacy & Security") follow the SYSTEM
/// language, not our UI language, so they move on any language change regardless of
/// `appearance.language`.
pub(crate) fn forget_system_language_caches() {
    crate::system_strings::invalidate();
}

/// The tag the user formats in, composed from the AUTOUPDATING current locale, the one Foundation
/// documents as tracking the user's preferences. `+[NSLocale currentLocale]` is a snapshot, and
/// this is read from a live-change path, so the autoupdating one removes any question of whether a
/// cached snapshot outlived the change that triggered the read.
pub(crate) fn format_locale() -> Option<String> {
    use objc2_foundation::NSLocale;

    let locale = NSLocale::autoupdatingCurrentLocale();
    // `countryCode` and ❌ never the better-named `regionCode`: that one is
    // macOS 14+, and calling it on 12 or 13 raises an unrecognized-selector
    // exception that unwinds out of an `extern "C"` frame and aborts the
    // process. Both read the same value, `rg` override included (`en_US@rg=sezzzz`
    // answers `SE` to either; macOS 26.5.2, Foundation via Swift, 2026-08-26),
    // and Foundation's own header names `regionCode` as this one's replacement.
    #[allow(deprecated, reason = "`regionCode`, the replacement, is macOS 14+ and we ship 12+")]
    let region = locale.countryCode();
    compose_format_locale(
        &locale.languageCode().to_string(),
        locale.scriptCode().map(|script| script.to_string()).as_deref(),
        region.map(|region| region.to_string()).as_deref()?,
    )
}
