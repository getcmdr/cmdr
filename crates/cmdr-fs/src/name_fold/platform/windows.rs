//! Windows: NTFS compares names case-insensitively but keeps two Unicode forms as two names,
//! so the key is the name lowercased, its form untouched.
//!
//! ❗ NTFS folds through its own upcase table, which disagrees with Unicode lowercasing on a
//! handful of characters (dotted and dotless I, final sigma). The drive index's Windows task
//! (windows-port plan, task 35) decides whether that edge is worth the volume's own table.

pub(crate) fn normalize_for_comparison(s: &str) -> String {
    s.to_lowercase()
}

pub(crate) fn compare_for_comparison(a: &str, b: &str) -> std::cmp::Ordering {
    if a.is_ascii() && b.is_ascii() {
        // ASCII lowercases byte by byte, so the common case compares without allocating.
        return a
            .bytes()
            .map(|c| c.to_ascii_lowercase())
            .cmp(b.bytes().map(|c| c.to_ascii_lowercase()));
    }
    normalize_for_comparison(a).cmp(&normalize_for_comparison(b))
}
