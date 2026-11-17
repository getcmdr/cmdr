//! The marker through `xattr`, shared by both Unixes. Only macOS writes a real one; Linux
//! answers the tests' stand-in name, so the reader is exercised there too.

pub(crate) fn read_domain_id_xattr(path: &str, name: &str) -> Option<String> {
    let raw = xattr::get(path, name).ok()??;
    String::from_utf8(raw).ok()
}
