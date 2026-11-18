//! No clock on Windows yet. Its reading is `GetThreadTimes(GetCurrentThread())`'s kernel plus
//! user time, which needs a Windows API dependency this crate doesn't carry; task 13 of
//! `docs/specs/windows-port-plan.md` owns it. Until then callers log no thread CPU.

pub(crate) fn current_thread_cpu_time() -> Option<std::time::Duration> {
    None
}
