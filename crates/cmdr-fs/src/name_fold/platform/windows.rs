//! Windows: NTFS compares names case-insensitively but keeps two Unicode forms as two names,
//! so the key is the name lowercased, its form untouched.
//!
//! ❗ NTFS folds through its own upcase table, which disagrees with Unicode lowercasing on a
//! handful of characters (dotted and dotless I, final sigma). The drive index's Windows task
//! (windows-port plan, task 35) decides whether that edge is worth the volume's own table.

pub(crate) fn normalize_for_comparison(s: &str) -> String {
    s.to_lowercase()
}
