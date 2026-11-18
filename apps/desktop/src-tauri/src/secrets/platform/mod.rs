//! Each OS's own secret store. A platform seam (`docs/guides/platform-seams.md`): `mod.rs` above
//! decides the dev and test overrides, and asks here only for the system store.

use super::SystemStore;

#[cfg_attr(target_os = "macos", path = "macos/mod.rs")]
#[cfg_attr(target_os = "linux", path = "linux/mod.rs")]
#[cfg_attr(target_os = "windows", path = "windows/mod.rs")]
mod imp;

// The instance-suffixed name the Keychain and Credential Manager file items under. Linux keeps
// the plain "Cmdr" service its Secret Service items have always had.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod service_name;

const _: fn() -> SystemStore = system_store;
const _: fn() -> String = item_name;

/// This OS's system store, chosen once at first access. ❌ Never a plaintext file: an OS whose
/// system store is out of reach falls back to an encrypted one (Linux) or to none at all.
pub(super) use imp::system_store;

/// What the person sees the store's items under, for pointing at leftovers in plain words: the
/// Keychain item name on macOS (instance-suffixed off prod), the Credential Manager target
/// prefix on Windows, else the plain product name.
pub(super) use imp::item_name;
