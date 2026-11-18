//! Linux: every thread under `/proc/self/task`, its name and tick-sampled split from
//! `stat`, its exact runtime from `schedstat`; the total and the event counts from
//! `getrusage` (`unix.rs`).
//!
//! **The runtime is exact, the user/system split is the kernel's estimate.** Without
//! `VIRT_CPU_ACCOUNTING` (every distro kernel) Linux learns user against system only from
//! the timer tick, then scales the tick counts to the scheduler's nanosecond runtime
//! (`cputime_adjust`); `stat` hands that out cut to whole ticks, 10 ms at `CLK_TCK` 100.
//! Summed over dozens of threads that cut would land in the census's exited share, so the
//! total comes from `schedstat`'s nanoseconds and is split by the same proportion the
//! kernel uses. A kernel without `schedstat` (`SCHED_INFO` off) falls back to the ticks.
//!
//! No wakeup counters: Linux has no `TASK_POWER_INFO`, and no syscall counts either.
//! Context switches and faults are `getrusage`'s, exited threads included.

use std::time::Duration;

use super::unix;
use crate::process_cpu::{CpuSnapshot, TaskCounters, ThreadSample};

pub(crate) fn take_snapshot() -> Option<CpuSnapshot> {
    let taken_at = crate::rt::now();
    let threads = list_thread_ids()?.into_iter().filter_map(read_thread).collect();
    let usage = unix::rusage_self()?;
    let (process_user, process_system) = unix::process_times(&usage);
    let count = |n: libc::c_long| n.max(0) as u64;
    Some(CpuSnapshot {
        taken_at,
        process_user,
        process_system,
        threads,
        wakeups: None,
        counters: Some(TaskCounters {
            context_switches: count(usage.ru_nvcsw) + count(usage.ru_nivcsw),
            unix_syscalls: None,
            mach_syscalls: None,
            faults: count(usage.ru_minflt) + count(usage.ru_majflt),
        }),
    })
}

fn list_thread_ids() -> Option<Vec<u64>> {
    let entries = match std::fs::read_dir("/proc/self/task") {
        Ok(entries) => entries,
        Err(e) => {
            log::debug!("process_cpu: reading /proc/self/task: {e}");
            return None;
        }
    };
    Some(
        entries
            .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
            .collect(),
    )
}

/// One thread's reading, or `None` if it exited between the listing and this read.
fn read_thread(tid: u64) -> Option<ThreadSample> {
    let stat = std::fs::read_to_string(format!("/proc/self/task/{tid}/stat")).ok()?;
    let (name, ticks) = parse_stat(&stat)?;
    let runtime = std::fs::read_to_string(format!("/proc/self/task/{tid}/schedstat"))
        .ok()
        .and_then(|text| parse_schedstat(&text));
    let (user, system) = match runtime {
        Some(runtime) => split(runtime, ticks),
        None => {
            let tick_ns = clock_tick_nanos()?;
            let of = |n: u64| Duration::from_nanos(n.saturating_mul(tick_ns));
            (of(ticks.user), of(ticks.system))
        }
    };
    Some(ThreadSample {
        id: tid,
        name: (!name.is_empty()).then_some(name),
        user,
        system,
    })
}

/// A thread's user and system tick counts from its `stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ticks {
    user: u64,
    system: u64,
}

/// The name and tick counts out of a `/proc/<pid>/task/<tid>/stat` line. The name sits
/// in parentheses and may itself hold spaces and parentheses, so the fields after it are
/// found from the LAST `)`.
fn parse_stat(stat: &str) -> Option<(String, Ticks)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let name = stat.get(open + 1..close)?.to_owned();
    // The first field after the name is field 3 (`state`); `utime` and `stime` are
    // fields 14 and 15 (`proc_pid_stat(5)`).
    let mut fields = stat.get(close + 1..)?.split_whitespace();
    let user = fields.nth(11)?.parse().ok()?;
    let system = fields.next()?.parse().ok()?;
    Some((name, Ticks { user, system }))
}

/// The on-CPU nanoseconds, the first field of `/proc/<pid>/task/<tid>/schedstat`.
fn parse_schedstat(text: &str) -> Option<Duration> {
    text.split_whitespace().next()?.parse().ok().map(Duration::from_nanos)
}

/// `runtime` split into user and system in the proportion of the tick counts, the way the
/// kernel's `cputime_adjust` does it: no system ticks means all user, no user ticks all
/// system.
fn split(runtime: Duration, ticks: Ticks) -> (Duration, Duration) {
    if ticks.system == 0 {
        return (runtime, Duration::ZERO);
    }
    if ticks.user == 0 {
        return (Duration::ZERO, runtime);
    }
    let nanos = runtime.as_nanos();
    let system = nanos * u128::from(ticks.system) / (u128::from(ticks.user) + u128::from(ticks.system));
    let to_duration = |n: u128| Duration::from_nanos(u64::try_from(n).unwrap_or(u64::MAX));
    (to_duration(nanos - system), to_duration(system))
}

/// One `stat` tick, in nanoseconds (10 ms at the usual `CLK_TCK` of 100).
fn clock_tick_nanos() -> Option<u64> {
    // SAFETY: `sysconf` reads a system constant and touches no memory of ours.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    (hz > 0).then(|| 1_000_000_000 / hz as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_fields_come_after_the_last_parenthesis_of_the_name() {
        let stat = "4242 (odd) name (x)) S 1 4242 4242 0 -1 4194368 120 0 0 0 37 5 0 0 20 0 9 0 1000 0 0";
        let (name, ticks) = parse_stat(stat).expect("well-formed");
        assert_eq!(name, "odd) name (x)");
        assert_eq!(ticks, Ticks { user: 37, system: 5 });
    }

    #[test]
    fn a_truncated_stat_line_reads_as_none() {
        assert_eq!(parse_stat("4242 (cmdr) S 1 2 3"), None);
        assert_eq!(parse_stat("4242 cmdr S"), None);
    }

    #[test]
    fn schedstat_is_the_first_field_in_nanoseconds() {
        assert_eq!(
            parse_schedstat("123456789 2000 17\n"),
            Some(Duration::from_nanos(123_456_789))
        );
        assert_eq!(parse_schedstat(""), None);
    }

    /// The exact runtime keeps its full value; only its split follows the ticks.
    #[test]
    fn the_runtime_splits_in_the_proportion_of_the_ticks() {
        let runtime = Duration::from_millis(1_000);
        assert_eq!(
            split(runtime, Ticks { user: 3, system: 1 }),
            (Duration::from_millis(750), Duration::from_millis(250))
        );
        assert_eq!(split(runtime, Ticks { user: 0, system: 0 }), (runtime, Duration::ZERO));
        assert_eq!(split(runtime, Ticks { user: 0, system: 4 }), (Duration::ZERO, runtime));
        let (user, system) = split(Duration::from_nanos(10), Ticks { user: 1, system: 2 });
        assert_eq!(
            user + system,
            Duration::from_nanos(10),
            "no nanosecond is lost to rounding"
        );
    }
}
