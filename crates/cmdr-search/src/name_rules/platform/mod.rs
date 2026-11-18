//! Picks the name rules of the machine this build targets.

use super::NameRules;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: NameRules = HOST;

/// How this OS's local filesystem compares names. Its `fold` gives the same key as
/// `cmdr_fs::name_fold::normalize_for_comparison`, which the drive index stores.
pub(super) use imp::HOST;
