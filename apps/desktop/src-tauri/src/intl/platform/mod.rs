//! Where each OS keeps the user's language and region, and how it says they moved. A platform
//! seam (`docs/guides/platform-seams.md`); the resolver it feeds (`../resolve.rs`), the format-tag
//! composer (`../format_locale.rs`), and the change watcher (`../live_locale.rs`) are portable.

use std::sync::Arc;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

/// The POSIX locale environment, read by the arm whose OS has no preference list of its own.
#[cfg(target_os = "linux")]
mod locale_env;

const _: fn() -> Option<String> = webview_ui_locale;
const _: fn() -> String = os_ui_locale;
const _: fn() -> Option<String> = format_locale;
const _: fn(Arc<dyn Fn() + Send + Sync>) = observe_locale_signals;
const _: fn() = forget_system_language_caches;
const _: fn() -> super::native_strings::NativeOs = native_text_os;

/// The tag whose conventions the user formats in (`en-SE`), or `None` when the OS has no usable
/// answer and the webview's own default should stand. Read fresh on every call.
pub(crate) use imp::format_locale;

/// Calls the callback on every OS signal that the language or region may have moved, for the
/// whole session. Unfiltered and bursty: `live_locale.rs`'s watcher does the filtering.
pub(crate) use imp::observe_locale_signals;

/// Drops whatever the OS side cached in the SYSTEM language (macOS's quoted pane labels), right
/// before a language change is announced.
pub(crate) use imp::forget_system_language_caches;

/// Which system's text a native string reads when its catalog value is a platform block (on
/// Linux, this session's desktop). `native_strings::pick` does the choosing; only this answer
/// is per-OS.
pub(crate) use imp::native_text_os;

/// The catalog the OS asks the WEBVIEW to open, or `None` when the OS has no answer of its own
/// and the webview's default should stand. Read fresh on every call, so a live change shows.
pub(crate) use imp::webview_ui_locale;

/// The catalog the NATIVE surfaces (the menu bar, the already-running alert) open while the user
/// hasn't pinned a language. Always an answer: they have no webview to defer to.
pub(crate) use imp::os_ui_locale;

#[cfg(test)]
mod tests {
    use super::super::native_strings::NativeOs;

    #[test]
    fn this_hosts_arm_answers_its_own_os_for_native_text() {
        // Each CI OS lane runs this against its own arm, Windows included: an arm copied from
        // its neighbour without changing the answer would have Windows read Linux words.
        let host = [
            (cfg!(target_os = "macos"), NativeOs::MacOs),
            (cfg!(target_os = "windows"), NativeOs::Windows),
            (
                cfg!(target_os = "linux"),
                NativeOs::Linux(crate::platform::LinuxDesktop::Other),
            ),
        ]
        .into_iter()
        .find_map(|(is_host, os)| is_host.then_some(os))
        .expect("the app builds only for macOS, Windows, and Linux");
        // Linux names the desktop too; the OS is what an arm can get wrong by copying.
        let os_only = |os: NativeOs| match os {
            NativeOs::Linux(_) => NativeOs::Linux(crate::platform::LinuxDesktop::Other),
            other => other,
        };
        assert_eq!(os_only(super::native_text_os()), host);
    }

    /// The real machine, not a table: whatever this host is set to, its format answer has to be a
    /// tag we'd hand `Intl` without flinching (or none at all, where the webview default stands).
    #[test]
    fn the_live_format_answer_is_a_well_formed_tag() {
        use super::super::format_locale::{is_language_subtag, is_region_subtag};

        let Some(tag) = super::format_locale() else {
            return;
        };
        let mut parts = tag.split('-');
        assert!(
            is_language_subtag(parts.next().unwrap_or_default()),
            "bad language in {tag}"
        );
        let last = parts.next_back().unwrap_or_default();
        assert!(is_region_subtag(last), "bad region in {tag}");
    }
}
