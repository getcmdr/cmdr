//! How each OS catches a crash the panic hook can't see, and the process facts a crash report
//! records. A platform seam (`docs/guides/platform-seams.md`); the report, its files, and the
//! next-launch pass are portable.

use std::path::Path;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

#[cfg(unix)]
mod unix;

/// Windows' capture: the unhandled-exception filter and the abort handler.
#[cfg(target_os = "windows")]
mod exception_filter;
/// The record that filter writes. Pure, so it's compiled (and tested) on every host.
#[cfg(any(target_os = "windows", test))]
pub(super) mod exception_record;

#[cfg(test)]
mod exception_record_tests;
// Windows only: crashes a child process through the real exception filter.
#[cfg(all(test, target_os = "windows"))]
mod exception_filter_tests;

const _: fn(&Path, &Path) = install_native_crash_hooks;
const _: fn(&Path) -> std::path::PathBuf = session_raw_crash_path;
const _: fn(&Path) -> bool = raw_crash_file_in_use;
const _: fn(&Path, &Path) = convert_raw_crash;
const _: fn() -> u64 = current_image_base;
const _: fn() -> usize = current_thread_count;
const _: fn() -> Option<Vec<String>> = module_backtrace_frames;

/// Arms this OS's native-crash capture: the raw crash file at `raw_crash_path`, plus any
/// sidecar under `data_dir`. Call only after the next-launch pass read the previous session's
/// files, never before.
pub(super) use imp::install_native_crash_hooks;

/// The raw crash file THIS session's capture writes, given the fixed `crash-report.raw` path. On
/// Unix that fixed path (one instance at a time); on Windows one per process
/// (`crash-report.<pid>-<start ms>.raw`), since two instances can run there and each must keep
/// its own record.
pub(super) use imp::session_raw_crash_path;

/// Whether a running process still holds this raw crash file, so the next-launch pass must leave
/// it alone. Always `false` on Unix, where only one instance writes.
pub(super) use imp::raw_crash_file_in_use;

/// Bytes of a well-formed raw crash in this OS's format, for the next-launch tests.
#[cfg(test)]
pub(super) use imp::sample_raw_crash;

/// Turns the raw file that capture left at `raw_crash_path` into a report at `crash_json_path`, at
/// the next launch. The caller deletes the raw file afterwards.
pub(super) use imp::convert_raw_crash;

/// The main image's load address in THIS process, or 0 when unknown. Only meaningful for a
/// report built in-process: a relaunched process has a different ASLR slide.
pub(super) use imp::current_image_base;

/// How many threads this process has right now, or 0 when the OS can't say.
pub(super) use imp::current_thread_count;

/// This thread's stack as `module+0xoffset` frames, for a panic whose own backtrace couldn't name
/// our code (a shipped Windows binary has no PDB beside it), or `None` where the OS's own
/// backtrace is all there is.
pub(super) use imp::module_backtrace_frames;
