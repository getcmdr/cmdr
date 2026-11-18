//! Windows: `GetThreadTimes` on the calling thread, its kernel plus user time. The two come in
//! `FILETIME`s, 100-nanosecond ticks split into two 32-bit halves.

use std::time::Duration;

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

pub(crate) fn current_thread_cpu_time() -> Option<Duration> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: `GetCurrentThread` returns a pseudo-handle that always names the calling thread
    // and needs no closing, and the four pointers are to live, writable `FILETIME`s in this
    // frame, which outlive the call.
    let ok = unsafe { GetThreadTimes(GetCurrentThread(), &mut creation, &mut exit, &mut kernel, &mut user) };
    (ok != 0).then(|| ticks_to_duration(ticks(kernel).saturating_add(ticks(user))))
}

/// A `FILETIME`'s 100-nanosecond ticks.
fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn ticks_to_duration(ticks: u64) -> Duration {
    Duration::from_nanos(ticks.saturating_mul(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filetime_reads_as_its_100_ns_ticks() {
        let time = FILETIME {
            dwLowDateTime: 5,
            dwHighDateTime: 1,
        };
        assert_eq!(ticks(time), (1 << 32) + 5);
        assert_eq!(ticks_to_duration(10_000_000), Duration::from_secs(1));
        assert_eq!(ticks_to_duration(u64::MAX), Duration::from_nanos(u64::MAX));
    }

    /// Cumulative and per-thread: it grows while this thread spins.
    #[test]
    fn the_calling_threads_time_grows_while_it_works() {
        let before = current_thread_cpu_time().expect("GetThreadTimes answers for the calling thread");
        let start = std::time::Instant::now();
        let mut spin = 0_u64;
        while start.elapsed() < Duration::from_millis(100) {
            spin = std::hint::black_box(spin.wrapping_add(1));
        }
        let after = current_thread_cpu_time().expect("GetThreadTimes answers for the calling thread");
        assert!(after > before, "{before:?} -> {after:?}");
    }
}
