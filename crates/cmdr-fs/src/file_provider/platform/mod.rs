//! Picks this OS's reader for the File Provider domain marker.

#[cfg(unix)]
mod unix;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: fn(&str, &str) -> Option<String> = read_domain_id_xattr;

/// The xattr `name` on `path` itself (never following a link), as UTF-8; `None` for a read
/// failure, a missing attribute, a non-UTF-8 value, or an OS with no File Provider marker.
pub(super) use imp::read_domain_id_xattr;
