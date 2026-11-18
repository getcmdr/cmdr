//! The process total both Unixes read the same way: `getrusage(RUSAGE_SELF)`, which keeps
//! exited threads' time. On Linux it's the scheduler's nanosecond runtime split by the
//! tick sample and handed out in microseconds, the same basis as the per-thread reading.

use std::time::Duration;

pub(super) fn rusage_self() -> Option<libc::rusage> {
    // SAFETY: an all-zero `rusage` is a valid value of that plain C struct.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `getrusage` writes one `rusage` through the pointer; `usage` is a live,
    // writable local of exactly that type. On failure it writes nothing we read.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return None;
    }
    Some(usage)
}

/// The process's cumulative user and system time out of one `getrusage` reading.
pub(super) fn process_times(usage: &libc::rusage) -> (Duration, Duration) {
    (timeval(usage.ru_utime), timeval(usage.ru_stime))
}

fn timeval(tv: libc::timeval) -> Duration {
    Duration::new(tv.tv_sec.max(0) as u64, 0) + Duration::from_micros(tv.tv_usec.max(0) as u64)
}
