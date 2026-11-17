//! Picks how this OS's local filesystem folds a name into the drive index's key.

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: fn(&str) -> String = normalize_for_comparison;

/// `s` folded the way this OS's local filesystem compares names.
pub(super) use imp::normalize_for_comparison;
