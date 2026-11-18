//! The browser build has no local filesystem; it keeps a name as its own key, as Linux does.

pub(crate) fn normalize_for_comparison(s: &str) -> String {
    s.to_string()
}

pub(crate) fn compare_for_comparison(a: &str, b: &str) -> std::cmp::Ordering {
    a.cmp(b)
}
