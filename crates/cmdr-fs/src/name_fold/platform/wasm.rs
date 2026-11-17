//! The browser build has no local filesystem; it keeps a name as its own key, as Linux does.

pub(crate) fn normalize_for_comparison(s: &str) -> String {
    s.to_string()
}
