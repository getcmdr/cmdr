//! Picks this OS's per-thread CPU clock.

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;
#[cfg(unix)]
mod unix;

const _: fn() -> Option<std::time::Duration> = current_thread_cpu_time;

/// The calling thread's cumulative user + system CPU time, or `None` without a clock.
pub(super) use imp::current_thread_cpu_time;
