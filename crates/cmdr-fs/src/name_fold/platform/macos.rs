//! macOS: APFS compares names form- and case-insensitively, so the key is NFD, then lowercase.

use unicode_normalization::UnicodeNormalization as _;

pub(crate) fn normalize_for_comparison(s: &str) -> String {
    s.nfd().collect::<String>().to_lowercase()
}
