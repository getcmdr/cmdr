//! Windows: `GlobalMemoryStatusEx` for the total and what's available, `GetPerformanceInfo` for
//! the non-paged kernel pool. Falls back to `sysinfo` when either call fails.
//!
//! The categories, mapped onto Task Manager's: wired is the non-paged pool (kernel memory that
//! can't be paged out, the closest thing to macOS's wired pages); free is "Available" (free,
//! zeroed, and standby pages, which Windows hands out on demand like macOS's file cache); app is
//! the rest of "In use".

use super::SystemMemoryInfo;
use windows_sys::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

pub(crate) fn system_memory_info() -> SystemMemoryInfo {
    match read() {
        Some((total, available, nonpaged)) => breakdown(total, available, nonpaged),
        None => super::super::sysinfo_breakdown(),
    }
}

/// Total physical, available physical, and non-paged pool bytes.
fn read() -> Option<(u64, u64, u64)> {
    // SAFETY: an all-zero `MEMORYSTATUSEX` is a valid value of this plain C struct.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = u32::try_from(size_of::<MEMORYSTATUSEX>()).ok()?;
    // SAFETY: `status` is live and writable, and `dwLength` names its size, as the call requires.
    if unsafe { GlobalMemoryStatusEx(&raw mut status) } == 0 {
        log::warn!("GlobalMemoryStatusEx failed: {}", std::io::Error::last_os_error());
        return None;
    }

    // SAFETY: an all-zero `PERFORMANCE_INFORMATION` is a valid value of this plain C struct.
    let mut performance: PERFORMANCE_INFORMATION = unsafe { std::mem::zeroed() };
    let size = u32::try_from(size_of::<PERFORMANCE_INFORMATION>()).ok()?;
    // SAFETY: `performance` is live and writable for `size` bytes, its own size.
    if unsafe { GetPerformanceInfo(&raw mut performance, size) } == 0 {
        log::warn!("GetPerformanceInfo failed: {}", std::io::Error::last_os_error());
        return None;
    }
    let nonpaged = (performance.KernelNonpaged as u64).saturating_mul(performance.PageSize as u64);
    Some((status.ullTotalPhys, status.ullAvailPhys, nonpaged))
}

/// The three non-overlapping categories, summing to `total` whatever the counters say (they're
/// sampled a moment apart, so they can disagree a little).
fn breakdown(total: u64, available: u64, nonpaged: u64) -> SystemMemoryInfo {
    let free_bytes = available.min(total);
    let wired_bytes = nonpaged.min(total - free_bytes);
    SystemMemoryInfo {
        total_bytes: total,
        wired_bytes,
        app_bytes: total - free_bytes - wired_bytes,
        free_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_categories_split_in_use_memory_and_sum_to_the_total() {
        let gib = 1024 * 1024 * 1024;
        let info = breakdown(8 * gib, 3 * gib, gib / 2);
        assert_eq!(info.free_bytes, 3 * gib);
        assert_eq!(info.wired_bytes, gib / 2);
        assert_eq!(info.app_bytes, 8 * gib - 3 * gib - gib / 2);
    }

    #[test]
    fn counters_that_disagree_still_sum_to_the_total() {
        let info = breakdown(100, 120, 50);
        assert_eq!((info.free_bytes, info.wired_bytes, info.app_bytes), (100, 0, 0));
        let info = breakdown(100, 90, 50);
        assert_eq!((info.free_bytes, info.wired_bytes, info.app_bytes), (90, 10, 0));
    }

    #[test]
    fn this_machine_reads_real_counters() {
        let (total, available, nonpaged) = read().expect("both calls answer on any Windows");
        assert!(total > 0 && available > 0 && available <= total);
        assert!(nonpaged > 0, "the kernel always holds some non-paged pool");
    }
}
