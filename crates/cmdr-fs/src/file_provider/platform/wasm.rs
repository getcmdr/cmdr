//! The browser build has no xattrs, so no File Provider domains.

pub(crate) fn read_domain_id_xattr(_path: &str, _name: &str) -> Option<String> {
    None
}
