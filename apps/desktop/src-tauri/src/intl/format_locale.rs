//! The conventions the app formats in: dates, number grouping, first day of week.
//!
//! macOS models language and region as two settings, and a person can
//! legitimately read US English while living in Sweden. Foundation carries that
//! second setting as a region override on the locale
//! (`AppleLocale = en_US@rg=sezzzz`), and it formats accordingly: `2026-08-19`
//! and `1 234 567,89`.
//!
//! WebKit doesn't. The webview resolves that same machine to plain `en-US` and
//! writes `08/19/2026` and `1,234,567.89`, and handing the extension back
//! explicitly doesn't help (`en-US-u-rg-sezzzz` resolves straight to `en-US`).
//! A real region SUBTAG does: `en-SE` reproduces Foundation's output exactly.
//! So we compose that tag rather than asking the webview.
//!
//! Windows keeps the two apart the same way (display language versus Regional
//! format), and WebView2 formats in the display language, so its arm composes
//! from the Regional format's locale name.
//!
//! The OS reads live in `platform/` (`format_locale()`, one answer per OS:
//! `None` on Linux, where WebKit reads the same environment the desktop formats
//! from); this file is the pure string work they share.

/// Composes the formatting tag from a Windows locale name
/// (`GetUserDefaultLocaleName`): `en-SE`, `sr-Latn-RS`, or `de-DE_phoneb`, whose
/// `_phoneb` names an alternate sort, not a convention, and is dropped. Any other
/// shape (a neutral `en`, a variant like `ca-ES-valencia`, a custom `x-` name)
/// answers `None`, so the webview default stands.
#[cfg(any(target_os = "windows", test))]
pub(super) fn format_locale_from_windows_name(name: &str) -> Option<String> {
    let name = name.split('_').next().unwrap_or_default();
    let parts: Vec<&str> = name.split('-').collect();
    match parts.as_slice() {
        [language, region] => compose_format_locale(language, None, region),
        [language, script, region] if is_script_subtag(script) => compose_format_locale(language, Some(script), region),
        _ => None,
    }
}

/// Composes `<language>[-Script]-REGION` from Foundation's three parts, or
/// `None` when any part is missing or malformed.
///
/// `None` is load-bearing: the caller falls back to the webview's own locale,
/// which is a working answer. A malformed tag is not — `Intl` would either
/// throw or quietly resolve to something nobody chose, so a part we don't
/// recognize means we don't answer.
///
/// The script rides along only when Foundation names one, which is exactly when
/// dropping it would change the answer (`zh-Hans` and `zh-Hant` format dates
/// differently). Foundation leaves it `nil` for every locale whose script is
/// implied by its language, so the common tag stays the short one.
/// Composed on macOS and Windows; the tests exercise it on every platform, which
/// is where the interesting cases live (this is pure string work with no OS in
/// it).
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(super) fn compose_format_locale(language: &str, script: Option<&str>, region: &str) -> Option<String> {
    let language = language.trim();
    let region = region.trim();
    if !is_language_subtag(language) || !is_region_subtag(region) {
        return None;
    }
    let mut tag = language.to_ascii_lowercase();
    if let Some(script) = script.map(str::trim).filter(|script| !script.is_empty()) {
        if !is_script_subtag(script) {
            return None;
        }
        tag.push('-');
        tag.push_str(&titlecase(script));
    }
    tag.push('-');
    tag.push_str(&region.to_ascii_uppercase());
    Some(tag)
}

/// Two or three letters, and not the `und` that means "no language here".
/// Composing `und-SE` would be a tag we can't defend; no answer is better.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(super) fn is_language_subtag(part: &str) -> bool {
    matches!(part.len(), 2 | 3) && part.chars().all(|c| c.is_ascii_alphabetic()) && !part.eq_ignore_ascii_case("und")
}

/// Four letters (`Hans`, `Latn`).
#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn is_script_subtag(part: &str) -> bool {
    part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic())
}

/// Two letters (`SE`) or three digits (the UN M49 codes, `419`).
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(super) fn is_region_subtag(part: &str) -> bool {
    (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
        || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
}

/// `hans` → `Hans`, the casing BCP-47 writes scripts in.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn titlecase(part: &str) -> String {
    let mut chars = part.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_the_region_override_this_machine_carries() {
        // The motivating case: US English, Swedish region. `en-SE` is the tag
        // that reproduces Foundation's `2026-08-19` and `1 234 567,89`; plain
        // `en-US` is what the webview would have said on its own.
        assert_eq!(compose_format_locale("en", None, "SE"), Some("en-SE".to_string()));
    }

    #[test]
    fn a_machine_with_no_override_composes_the_tag_it_already_had() {
        // Nothing changes for the common case: an en-US Mac still formats en-US.
        assert_eq!(compose_format_locale("en", None, "US"), Some("en-US".to_string()));
        assert_eq!(compose_format_locale("hu", None, "HU"), Some("hu-HU".to_string()));
    }

    #[test]
    fn a_named_script_rides_along() {
        // Dropping it would hand `zh-Hant` readers Simplified date and number
        // conventions.
        assert_eq!(
            compose_format_locale("zh", Some("Hant"), "TW"),
            Some("zh-Hant-TW".to_string())
        );
    }

    #[test]
    fn casing_comes_out_canonical() {
        assert_eq!(
            compose_format_locale("ZH", Some("hant"), "tw"),
            Some("zh-Hant-TW".to_string())
        );
    }

    #[test]
    fn a_numeric_region_is_a_region() {
        // UN M49 codes are legal region subtags and Foundation does emit them.
        assert_eq!(compose_format_locale("es", None, "419"), Some("es-419".to_string()));
    }

    #[test]
    fn a_missing_or_malformed_part_answers_nothing() {
        // Every one of these would compose a tag `Intl` either rejects or
        // resolves to something the user never chose. The caller falls back to
        // the webview's own locale instead, which at least works.
        assert_eq!(compose_format_locale("", None, "SE"), None);
        assert_eq!(compose_format_locale("en", None, ""), None);
        assert_eq!(compose_format_locale("english", None, "SE"), None);
        assert_eq!(compose_format_locale("en", None, "SWE"), None);
        assert_eq!(compose_format_locale("en", None, "S"), None);
        assert_eq!(compose_format_locale("en", Some("Han"), "TW"), None);
        assert_eq!(compose_format_locale("e n", None, "SE"), None);
    }

    #[test]
    fn an_unknown_language_answers_nothing() {
        // `und` is Foundation saying it doesn't know. `und-SE` is a tag we
        // can't defend, so we'd rather say nothing and let the webview answer.
        assert_eq!(compose_format_locale("und", None, "SE"), None);
    }

    #[test]
    fn a_windows_locale_name_composes_the_same_tag() {
        // Windows' Regional format is a BCP-47-shaped name already (`GetUserDefaultLocaleName`):
        // English (Sweden) is `en-SE`, the same tag macOS's region override composes.
        assert_eq!(format_locale_from_windows_name("en-SE"), Some("en-SE".to_string()));
        assert_eq!(format_locale_from_windows_name("hu-HU"), Some("hu-HU".to_string()));
        assert_eq!(
            format_locale_from_windows_name("sr-Latn-RS"),
            Some("sr-Latn-RS".to_string())
        );
        assert_eq!(format_locale_from_windows_name("es-419"), Some("es-419".to_string()));
    }

    #[test]
    fn a_windows_sort_order_suffix_is_dropped() {
        // An alternate sort rides on the name after `_` (German phone book order): it sorts,
        // it doesn't format, and `Intl` would reject it.
        assert_eq!(
            format_locale_from_windows_name("de-DE_phoneb"),
            Some("de-DE".to_string())
        );
    }

    #[test]
    fn a_windows_name_without_a_region_or_with_extra_parts_answers_nothing() {
        // A neutral name has no conventions of its own, and a custom or variant name is one we
        // can't vouch for: the webview default stands.
        assert_eq!(format_locale_from_windows_name("en"), None);
        assert_eq!(format_locale_from_windows_name(""), None);
        assert_eq!(format_locale_from_windows_name("ca-ES-valencia"), None);
        assert_eq!(format_locale_from_windows_name("x-custom-locale"), None);
        assert_eq!(format_locale_from_windows_name("en-Latn"), None);
    }

    #[test]
    fn an_empty_script_is_the_same_as_no_script() {
        assert_eq!(compose_format_locale("en", Some(""), "SE"), Some("en-SE".to_string()));
    }
}
