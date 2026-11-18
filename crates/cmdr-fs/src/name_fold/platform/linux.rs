//! Linux: ext4 and friends are case- and form-sensitive, so a name is its own key; folding
//! would merge rows that are genuinely distinct files.

pub(crate) fn normalize_for_comparison(s: &str) -> String {
    s.to_string()
}
