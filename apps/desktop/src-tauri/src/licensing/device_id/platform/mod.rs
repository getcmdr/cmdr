//! Where each OS keeps the machine's own id, the raw value `device_id` salts and hashes. A
//! platform seam (`docs/guides/platform-seams.md`).

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn() -> Option<String> = read_platform_uuid;

/// This machine's raw id, unhashed, or `None` when the OS won't give a usable one (each arm logs
/// why). Stable across launches; ❗ it leaves this process only through `hash_platform_uuid`.
pub(super) use imp::read_platform_uuid;
