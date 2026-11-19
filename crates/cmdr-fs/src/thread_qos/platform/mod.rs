//! Picks this OS's way of lowering the calling thread.

use super::QosClass;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: fn(QosClass) = apply;

/// Moves the calling thread to `class` for the rest of its life. Never fails loudly: a refusal
/// is logged at debug, so a worker never aborts mid-scan over a scheduling hint.
pub(super) use imp::apply;
