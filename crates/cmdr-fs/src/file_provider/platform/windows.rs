//! Windows has no File Provider domains. Its sync engines (OneDrive and friends) use Cloud Files
//! placeholders, which the windows-port plan's task 37 reads its own way.

pub(crate) fn read_domain_id_xattr(_path: &str, _name: &str) -> Option<String> {
    None
}
