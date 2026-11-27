//! Raises the process's soft open-file limit (`RLIMIT_NOFILE`) once, at startup.
//!
//! A macOS app launched from the Dock or Finder inherits `launchd`'s soft limit, usually 256,
//! and a Linux desktop session's is usually 1,024. Cmdr holds many descriptors at once (SQLite
//! databases and their WAL files, file watchers, SMB and other network sessions, folder walks,
//! open listings), so the default cap fails ordinary operations with "too many open files"
//! (`EMFILE`). The hard limit is far higher on both, and any process may raise its soft limit up
//! to its hard one, so the app does that before it opens anything.
//!
//! The target is [`soft_limit_target`]: the lowest of the hard limit, macOS's
//! `kern.maxfilesperproc`, and [`TARGET`], and ❌ never below the current soft limit (a shell
//! that launched a dev build with a higher one keeps it). macOS refuses `setrlimit` with
//! `RLIM_INFINITY` or a value above `kern.maxfilesperproc` (`EINVAL`, the `OPEN_MAX` semantics
//! of `setrlimit(2)`'s compatibility notes), so the clamp there is load-bearing, not tidiness.
//!
//! Per OS in `platform/`: macOS and Linux share the POSIX raise (`unix.rs`), macOS adding its
//! `kern.maxfilesperproc` ceiling; Windows has nothing to raise (`windows.rs` says why). The
//! browser build has no process limits at all, so the module is host-only.
//!
//! A refusal is logged and ignored: the app runs on, at the limit it had.

/// The soft limit Cmdr asks for: Darwin's `OPEN_MAX` (`sys/syslimits.h`), 40× the Dock's 256, room for every database, watcher, session, and walk Cmdr runs at
/// once, while a descriptor leak still meets a cap (and an `EMFILE` that names it) instead of
/// running up to Linux's half-million hard limit.
pub const TARGET: u64 = 10_240;

/// What [`raise`] found and did, for the app to log once its logger is up (it runs before).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenFileLimit {
    /// The soft limit went from `from` to `to`.
    Raised {
        /// The soft limit the process started with.
        from: u64,
        /// The soft limit it has now.
        to: u64,
    },
    /// The soft limit was already at or above the target; nothing changed.
    AlreadyEnough {
        /// The soft limit the process started with, and still has.
        current: u64,
    },
    /// The OS refused to read or set the limit; the process keeps the one it had.
    Refused {
        /// The soft limit the process started with, when it could be read.
        current: Option<u64>,
        /// The target the process asked for, when it got that far.
        target: Option<u64>,
        /// The `errno` the refusing call left.
        errno: Option<i32>,
    },
    /// This OS has no such limit to raise (Windows).
    NotApplicable,
}

impl OpenFileLimit {
    /// Logs the outcome: `info` for a raise or a limit already high enough, `warn` for a refusal.
    pub fn log(&self) {
        match *self {
            Self::Raised { from, to } => {
                // allowed-log-private: descriptor counts from `getrlimit`, which name nothing of the user's
                log::info!(target: "open_file_limit", "Raised the soft open-file limit from {from} to {to}");
            }
            Self::AlreadyEnough { current } => {
                log::info!(target: "open_file_limit", "Kept the soft open-file limit at {current}, already enough");
            }
            Self::Refused { current, target, errno } => {
                log::warn!(
                    target: "open_file_limit",
                    // allowed-log-private: descriptor counts and an errno, which name nothing of the user's
                    "Couldn't raise the soft open-file limit (current {current:?}, target {target:?}, errno {errno:?}); keeping it"
                );
            }
            Self::NotApplicable => {}
        }
    }
}

/// The soft limit to set, from the current soft and hard limits and the OS's per-process ceiling
/// (`kern.maxfilesperproc` on macOS, `None` where there's none to honor). Never below `current`,
/// never above `hard` or `per_process_max`. Pure, so the tests inject every value.
pub fn soft_limit_target(current: u64, hard: u64, per_process_max: Option<u64>) -> u64 {
    let ceiling = per_process_max.map_or(hard, |max| hard.min(max));
    TARGET.min(ceiling).max(current)
}

/// Raises the soft open-file limit toward [`soft_limit_target`]. Call once, at the very top of
/// startup, before anything opens a file; log the answer with [`OpenFileLimit::log`].
pub fn raise() -> OpenFileLimit {
    platform::raise()
}

mod platform;

#[cfg(test)]
mod tests;
