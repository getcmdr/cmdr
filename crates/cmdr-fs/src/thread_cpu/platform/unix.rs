//! The POSIX per-thread CPU clock, which macOS and Linux spell the same way.
//!
//! **`CLOCK_THREAD_CPUTIME_ID`, not Mach `thread_info`.** Both give per-thread cumulative
//! CPU on macOS; the POSIX clock is one call with no port to manage and no `count`-word
//! protocol to get wrong. On Linux it reads the scheduler's nanosecond runtime, so it's
//! exact where `/proc/thread-self/stat` is tick-sampled. Verified per-thread rather than
//! per-process by the tests below.

use std::time::Duration;

pub(crate) fn current_thread_cpu_time() -> Option<Duration> {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `clock_gettime` writes a `timespec` through the pointer we hand it.
    // `ts` is a live, writable local of exactly that type, and
    // `CLOCK_THREAD_CPUTIME_ID` is a valid clock id on macOS 10.12+ and Linux 2.6.12+.
    // The call reads no Rust-side state and cannot invalidate any. A non-zero return
    // means it wrote nothing, which the check below honors.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if rc != 0 {
        return None;
    }
    Some(Duration::new(
        ts.tv_sec.max(0) as u64,
        ts.tv_nsec.clamp(0, 999_999_999) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counter must move with work done ON THIS THREAD and stay monotone.
    #[test]
    fn burning_cpu_advances_the_calling_threads_counter() {
        let before = current_thread_cpu_time().expect("CLOCK_THREAD_CPUTIME_ID must be readable");
        let mut sink = 0u64;
        for i in 0..20_000_000u64 {
            sink = sink.wrapping_add(i).wrapping_mul(2_654_435_761);
        }
        std::hint::black_box(sink);
        let after = current_thread_cpu_time().expect("second read");
        assert!(
            after > before,
            "thread CPU time must advance after real work: {before:?} → {after:?}"
        );
    }

    /// The whole reason this exists: it must attribute CPU to ONE thread. A
    /// thread that only waits must not accumulate a busy sibling's time, or the
    /// writer's number would just be the process's.
    #[test]
    fn a_sibling_threads_work_does_not_land_on_this_threads_counter() {
        let before = current_thread_cpu_time().expect("first read");
        std::thread::spawn(|| {
            let mut sink = 0u64;
            for i in 0..200_000_000u64 {
                sink = sink.wrapping_add(i).wrapping_mul(2_654_435_761);
            }
            std::hint::black_box(sink);
        })
        .join()
        .expect("worker thread panicked");
        let after = current_thread_cpu_time().expect("second read");
        assert!(
            after.saturating_sub(before) < Duration::from_millis(50),
            "a joining thread must not be charged the worker's CPU: {before:?} → {after:?}"
        );
    }
}
