//! How much CPU time the CALLING thread has burned since it started.
//!
//! A dedicated background thread's CPU cost is invisible from outside the
//! process on macOS: `ps -M` prints per-thread cumulative CPU but no thread
//! names, so a profile can't tell the index writer from a tokio worker. The only
//! honest way to attribute CPU to one named thread is to have that thread read
//! its own counter and report it. This is that read.
//!
//! **Cumulative, never a rate.** The value only grows, so a caller measuring a
//! window subtracts two readings. That's deliberate: a sampled instantaneous rate
//! is exactly the instrument that produced a wrong answer for this codebase
//! before (`docs/notes/size-only-subtrees-before-baseline-2026-08-06.md` § "CPU
//! and memory under churn": a 20-second `sample` put a thread at 45% of CPU and
//! a cumulative reading refuted it at 3.4%). A difference of two monotone
//! readings cannot lie that way.
//!
//! Per OS in `platform/`: `CLOCK_THREAD_CPUTIME_ID` on macOS and Linux, `GetThreadTimes`
//! on Windows, none in the browser, where callers degrade to "no counter", never to a
//! wrong number.

mod platform;

use std::time::Duration;

/// The calling thread's cumulative CPU time (user + system) since it started, or
/// `None` if the platform has no per-thread clock or the read failed.
///
/// Cheap enough to call on a logging heartbeat: one `clock_gettime`, no
/// allocation. Don't call it per message: the point is a per-window delta, and
/// the syscall would then be a real fraction of what it measures.
pub fn current_thread_cpu_time() -> Option<Duration> {
    platform::current_thread_cpu_time()
}
