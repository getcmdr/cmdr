//! The real readers against a real kernel, on every OS that has one.

use std::sync::mpsc;
use std::time::Duration;

use super::take_snapshot;
use crate::process_cpu::CpuCensus;

/// Spends `cpu` of THIS thread's CPU time, by its own CPU clock: a fixed iteration count
/// costs a debug build 10× its CPU in wall clock on a loaded machine and ran past the 8 s
/// nextest cap, where a CPU budget stays a CPU budget.
fn burn(cpu: Duration) {
    let thread_cpu = || crate::thread_cpu::current_thread_cpu_time().expect("the thread CPU clock reads");
    let until = thread_cpu() + cpu;
    let mut sink = 0u64;
    while thread_cpu() < until {
        for i in 0..100_000u64 {
            sink = sink.wrapping_add(i).wrapping_mul(2_654_435_761);
        }
    }
    std::hint::black_box(sink);
}

/// The calling thread's kernel ID, the one a snapshot lists it under.
fn own_thread_id() -> u64 {
    #[cfg(target_os = "macos")]
    {
        let mut id = 0u64;
        // SAFETY: a null thread asks for the calling thread's ID, written through `id`, a
        // live, writable `u64` local.
        let read = unsafe { libc::pthread_threadid_np(0 as libc::pthread_t, &mut id) };
        assert_eq!(read, 0, "pthread_threadid_np reads the calling thread's ID");
        id
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `gettid` takes no arguments and only returns the caller's TID.
        let tid = unsafe { libc::gettid() };
        u64::try_from(tid).expect("a TID is positive")
    }
}

/// Twice the 20 ms both cells assert, so the census's rounding can't sink it.
const BURN: Duration = Duration::from_millis(40);

/// Short enough for Linux's 15-byte `comm`, which would cut a longer name.
const PROBE_NAME: &str = "cpu-probe";

/// The whole point: a named thread's work lands on THAT name, and nowhere else.
#[test]
fn a_named_busy_thread_shows_up_with_its_name_and_its_cpu() {
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let worker = std::thread::Builder::new()
        .name(PROBE_NAME.into())
        .spawn(move || {
            go_rx.recv().expect("go");
            burn(BURN);
            done_tx.send(()).expect("done");
            stop_rx.recv().expect("stop");
        })
        .expect("spawn");

    // `std::thread::Builder::name` names the pthread from inside the new thread, so wait
    // until the kernel sees the name before the first reading.
    let mut start = None;
    crate::testing::wait_until(Duration::from_secs(2), "the probe thread appears with its name", || {
        start = take_snapshot().filter(|snap| snap.threads.iter().any(|t| t.name.as_deref() == Some(PROBE_NAME)));
        start.is_some()
    });
    let start = start.expect("wait_until returned, so the named reading is there");

    go_tx.send(()).expect("go");
    done_rx.recv().expect("done");
    let end = take_snapshot().expect("second reading");
    stop_tx.send(()).expect("stop");
    worker.join().expect("worker");

    let census = CpuCensus::between(&start, &end, 1_000);
    let probe = census
        .threads
        .iter()
        .find(|t| t.name.as_deref() == Some(PROBE_NAME))
        .expect("the probe is live at the second reading");
    assert!(
        probe.cpu.cpu_us > 20_000,
        "40 ms of burning must show on the probe: {probe:?}"
    );
    assert!(
        census.process.cpu_us >= probe.cpu.cpu_us,
        "the process delta includes every thread: {census:?}"
    );
    assert!(
        census.counters.is_some_and(|c| c.context_switches > 0),
        "the task counters must be readable, and the probe's waits switched context: {census:?}"
    );
    #[cfg(target_os = "macos")]
    assert!(census.wakeups.is_some(), "TASK_POWER_INFO must be readable");
}

/// A thread that exits between the readings still shows up, as the process delta the
/// live threads don't explain.
///
/// ❗ `join` returning doesn't mean the kernel has let go of the thread: on macOS its Mach
/// thread tears down after the pthread signals its exit, so `PROC_PIDLISTTHREADIDS` can
/// still list it for a moment, and under load that moment is long enough to land in the
/// second reading (CI, 8 `raced` verdicts 2026-10-26 to 2026-10-28). Linux wakes the
/// joiner from the exiting thread's own teardown, before `/proc` drops it. So the second
/// reading is the first one the kernel no longer lists it in.
#[test]
fn a_thread_that_exits_inside_the_window_lands_in_exited_cpu() {
    let start = take_snapshot().expect("first reading");
    let churn = std::thread::Builder::new()
        .name("cpu-churn".into())
        .spawn(|| {
            burn(BURN);
            own_thread_id()
        })
        .expect("spawn")
        .join()
        .expect("churn thread");
    // By ID, ❌ not by name: a thread on its way out may already list without one.
    let mut end = None;
    crate::testing::wait_until(Duration::from_secs(5), "the kernel lets go of the churn thread", || {
        end = take_snapshot().filter(|snap| snap.threads.iter().all(|t| t.id != churn));
        end.is_some()
    });
    let end = end.expect("wait_until returned, so a reading without the churn thread is there");

    let census = CpuCensus::between(&start, &end, 1_000);
    assert!(
        census.exited_threads.cpu_us > 20_000,
        "the exited thread's work must land in exitedThreads: {census:?}"
    );
}
