//! Thread scheduling priority (QoS) hints for background work.
//!
//! macOS schedules threads by QoS ("quality of service") class: under CPU contention a
//! `USER_INTERACTIVE` / `USER_INITIATED` thread preempts a `UTILITY` / `BACKGROUND` one,
//! and the class also steers I/O throttling and CPU-frequency selection. Cmdr's UI and
//! IPC handlers run at the process-default (user-interactive) tier; the heavy background
//! indexing threads lower themselves to `UTILITY` so a runaway or failing scan can never
//! starve the webview for CPU. This is the in-process safety net that complements the
//! per-subsystem fixes (fatal errors stop instead of retry-looping, bounded logging).
//!
//! Per OS in `platform/`:
//!
//! - **macOS**: `pthread_set_qos_class_self_np(QOS_CLASS_UTILITY)`.
//! - **Windows**: the two halves of macOS's class, separately: `THREAD_PRIORITY_BELOW_NORMAL`
//!   (the UI's threads preempt it) plus EcoQoS, the execution-speed power throttle (Windows 11
//!   steers it to efficient cores and frequencies; Windows 10 calls the same flag LowQoS). ❌
//!   Never `THREAD_MODE_BACKGROUND_BEGIN`: it also drops the thread's I/O and memory priority
//!   to very low, Windows' twin of macOS's `Background` tier, which would make a scan crawl.
//! - **Linux and the browser**: nothing. We rely on the scheduler's fair share; `setpriority(2)`
//!   (a per-thread nice) would slot in behind the same API if it's ever wanted.
//!
//! ## Why `Utility` and not `Background`
//!
//! We expose only `Utility`. macOS also has a `Background` tier, but it's aggressively
//! I/O-throttled: it would make scans crawl and interact badly with disk throughput. Since
//! every background indexing thread bears user-visible progress, `Utility` (below the UI,
//! but a fair CPU share and normal I/O) is the right tier for all of them. `Background`
//! would only fit a genuinely idle maintenance tick with nobody waiting; we have none
//! today, so it isn't wired up (add the variant when a real use appears).
//!
//! ## Usage discipline
//!
//! Call [`set_current_thread_qos`] once, at the very top of a **dedicated** background
//! thread's body. Never call it on a pooled/shared thread (a tokio worker, a rayon
//! worker): the change persists for the thread's whole lifetime, so it would leak a
//! lowered priority onto later, unrelated tasks that land on the same thread. The
//! indexing live/replay loops run as tokio tasks and are deliberately NOT lowered here
//! for exactly this reason.
//!
//! A pool whose every thread runs only `Utility` work is the exception, because nothing
//! unrelated ever lands on it: [`crate::utility_pool::UtilityPool`] lowers each of its
//! threads once, at the top, the same way a dedicated thread does.
//!
//! ## ❌ Never lower a thread a UI-facing thread waits on
//!
//! A lowered thread holding a lock (or owing a reply) that the UI, an IPC handler, or anything
//! else a person is waiting on needs, turns "yield to the UI" into "the UI waits for the
//! background". macOS softens this: a waiter on a pthread mutex or `os_unfair_lock` lends the
//! holder its QoS. Windows doesn't for user-mode locks (SRW locks, which `std` uses, and channels
//! carry no priority at all): a below-normal holder that normal-priority work keeps off the CPU
//! runs again only when the balance-set manager's anti-starvation boost reaches it, after about 4 s.
//! So a thread is lowered only when every lock it shares is shared with other background threads,
//! and every wait on it is a background wait.
//!
//! The lowered threads today, all in `cmdr-index` (checked 2026-11-19): the index writer, the
//! scanner, the walker and rescan [`crate::utility_pool::UtilityPool`]s, the local reconcile, the
//! phase and cover walks. None qualifies as waited on:
//!
//! - **The index writer** owns its write connection; the UI reads through its own WAL read
//!   connections, which never wait on a writer. What waits on the writer (`flush`, its one reply
//!   message) is scan bookkeeping on background tasks, ❌ never an IPC handler.
//! - **The walker's locks** (its queue, slots, worker count, watchdog) are its own pool's; the
//!   rescan reconciler's `pending_rescans` is shared with the watcher's event loop, a background
//!   task. No app path takes either.
//!
//! A new lowered thread, or a new lock or reply a lowered thread shares with the UI's side, needs
//! this check again.
//!
//! ## No-op in test builds
//!
//! In any test build this is a no-op. Unit tests run massively parallel under nextest
//! (roughly one process per core), so lowering a heavy background thread there can starve
//! it past a per-test timeout, with no UI to protect and no production meaning.
//!
//! The condition is `cfg(test)` **or** the `testing` feature, and the feature half is
//! load-bearing: a consumer's `#[test]`s compile this crate as a plain dependency, where
//! `cfg(test)` is NOT set, so `cfg(test)` alone would silently start applying the real
//! QoS to every caller's tests. Consumers switch the feature on through a
//! dev-dependency, which keeps it off in production and dev builds — those get the real
//! class. Each arm's FFI is still verified directly by its own test, which calls the arm.

mod platform;

/// Scheduling tier for a dedicated background thread. Maps to a macOS `qos_class_t`, and on
/// Windows to a below-normal priority with EcoQoS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QosClass {
    /// User-initiated, progress-bearing work the user is (loosely) waiting on, but that
    /// must yield to the UI. Indexing shows a progress indicator, so this is its tier:
    /// below user-interactive, yet still a fair CPU share and normal (unthrottled) I/O.
    Utility,
}

/// Lowers the calling thread's scheduling priority to `class`.
///
/// See the module docs for the call-site discipline (dedicated threads only, ❌ never one a
/// UI-facing thread waits on) and the test-build no-op.
pub fn set_current_thread_qos(class: QosClass) {
    // Real work only in non-test builds. See the module docs for why tests skip it, and why
    // the `testing` feature has to be in the condition alongside `test`.
    if cfg!(any(test, feature = "testing")) {
        return;
    }
    platform::apply(class);
}
