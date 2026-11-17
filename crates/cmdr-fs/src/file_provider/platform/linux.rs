//! Linux has no File Provider, but reads xattrs the Unix way, so the reader's tests run here.

pub(crate) use super::unix::read_domain_id_xattr;
