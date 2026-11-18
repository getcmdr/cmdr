//! The browser build has no thread CPU clock to read.

pub(crate) fn current_thread_cpu_time() -> Option<std::time::Duration> {
    None
}
