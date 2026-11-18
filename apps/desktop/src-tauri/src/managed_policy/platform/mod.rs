//! Where each OS keeps an organization's policy, and what tells Cmdr it changed. A platform seam
//! (`docs/guides/platform-seams.md`); the sources answer `ManagedPrefsSource`, so the one parse in
//! `keys.rs` reads every OS the same way.

use super::source::ManagedPrefsSource;

#[cfg_attr(target_os = "macos", path = "macos/mod.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows/mod.rs")]
mod imp;

// Registry values as policy values, pure, so its rules run on every OS's tests.
#[cfg(any(target_os = "windows", test))]
pub(super) mod registry_values;

const _: fn() -> Box<dyn ManagedPrefsSource> = system_source;
const _: fn() = start_refresh_triggers;

/// This OS's managed layer, opened for one read pass. ❗ Only FORCED values: a user's own
/// preference must never read as policy.
pub(super) use imp::system_source;

/// Starts what re-reads the policy when the OS's store changes (each trigger calls
/// `managed_policy::refresh`). Called once from `init`.
pub(super) use imp::start_refresh_triggers;
