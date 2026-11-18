//! Picks this OS's process CPU reader.

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;
#[cfg(unix)]
mod unix;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod live_tests;

const _: fn() -> Option<super::CpuSnapshot> = take_snapshot;

/// Every live thread's name and cumulative CPU, the process total, and whatever event
/// counters this OS keeps, or `None` if the per-thread read failed or the OS has none.
pub(super) use imp::take_snapshot;
