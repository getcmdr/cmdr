//! Windows: macOS's `Utility` class in Windows' two pieces. `THREAD_PRIORITY_BELOW_NORMAL` is
//! the "yield to the UI" half: the webview's and the IPC handlers' normal-priority threads
//! preempt it under contention. EcoQoS (`ThreadPowerThrottling` with the execution-speed flag)
//! is the energy half: Windows 11 runs the thread on efficient cores at efficient clocks, as
//! macOS does `Utility` work on Apple silicon; Windows 10 1709+ takes the same flag as LowQoS.
//!
//! ❌ Never `THREAD_MODE_BACKGROUND_BEGIN`: it also drops I/O and memory priority to very low,
//! macOS's `Background` tier, which `super::super`'s docs rule out for progress-bearing work.

use std::ffi::c_void;

use windows_sys::Win32::System::Threading::{
    GetCurrentThread, SetThreadInformation, SetThreadPriority, THREAD_POWER_THROTTLING_CURRENT_VERSION,
    THREAD_POWER_THROTTLING_EXECUTION_SPEED, THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY,
    THREAD_PRIORITY_BELOW_NORMAL, ThreadPowerThrottling,
};

use super::QosClass;

/// What one class asks of Windows: a priority, and whether the thread runs under EcoQoS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsTier {
    priority: THREAD_PRIORITY,
    eco_qos: bool,
}

fn tier(class: QosClass) -> WindowsTier {
    match class {
        QosClass::Utility => WindowsTier {
            priority: THREAD_PRIORITY_BELOW_NORMAL,
            eco_qos: true,
        },
    }
}

/// The power-throttling state that turns EcoQoS on (`eco_qos`) or explicitly off for a thread.
fn throttling_state(eco_qos: bool) -> THREAD_POWER_THROTTLING_STATE {
    THREAD_POWER_THROTTLING_STATE {
        Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: if eco_qos {
            THREAD_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
    }
}

pub(crate) fn apply(class: QosClass) {
    // allowed-discarded-outcome: a refusal is already logged, and a scheduling hint never fails a worker
    let _ = apply_tier(class);
}

/// What Windows accepted of a tier: the priority, and the power throttle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Applied {
    priority: bool,
    eco_qos: bool,
}

fn apply_tier(class: QosClass) -> Applied {
    let tier = tier(class);
    // SAFETY: `GetCurrentThread` is a pseudo-handle for the calling thread that needs no
    // closing, and `tier.priority` is one of the documented `THREAD_PRIORITY_*` levels.
    let priority = unsafe { SetThreadPriority(GetCurrentThread(), tier.priority) } != 0;
    if !priority {
        log::debug!(
            target: "thread_qos",
            "SetThreadPriority(class={class:?}) refused: {}",
            std::io::Error::last_os_error()
        );
    }
    let state = throttling_state(tier.eco_qos);
    // SAFETY: the pseudo-handle names the calling thread; `state` is a live, correctly sized
    // `THREAD_POWER_THROTTLING_STATE`, which is what `ThreadPowerThrottling` reads, and the
    // call only reads it for its duration.
    let set = unsafe {
        SetThreadInformation(
            GetCurrentThread(),
            ThreadPowerThrottling,
            (&raw const state).cast::<c_void>(),
            size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if set == 0 {
        log::debug!(
            target: "thread_qos",
            "EcoQoS (class={class:?}) refused: {}",
            std::io::Error::last_os_error()
        );
    }
    Applied {
        priority,
        eco_qos: set != 0,
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
    use windows_sys::Win32::System::Threading::{GetThreadInformation, GetThreadPriority, THREAD_PRIORITY_NORMAL};

    use super::*;

    fn current_priority() -> i32 {
        // SAFETY: the pseudo-handle names the calling thread.
        unsafe { GetThreadPriority(GetCurrentThread()) }
    }

    /// The calling thread's power-throttling state as Windows recorded it, or the error when
    /// this Windows can't read it back.
    fn current_throttling() -> std::io::Result<THREAD_POWER_THROTTLING_STATE> {
        let mut state = THREAD_POWER_THROTTLING_STATE {
            Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: 0,
            StateMask: 0,
        };
        // SAFETY: the pseudo-handle names the calling thread; `state` is a live, writable,
        // correctly sized `THREAD_POWER_THROTTLING_STATE` with its version set, as the class
        // requires.
        let ok = unsafe {
            GetThreadInformation(
                GetCurrentThread(),
                ThreadPowerThrottling,
                (&raw mut state).cast::<c_void>(),
                size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(state)
        }
    }

    #[test]
    fn utility_is_below_normal_with_eco_qos() {
        assert_eq!(
            tier(QosClass::Utility),
            WindowsTier {
                priority: THREAD_PRIORITY_BELOW_NORMAL,
                eco_qos: true
            }
        );
        let on = throttling_state(true);
        assert_eq!(
            (on.ControlMask, on.StateMask),
            (
                THREAD_POWER_THROTTLING_EXECUTION_SPEED,
                THREAD_POWER_THROTTLING_EXECUTION_SPEED
            )
        );
        let off = throttling_state(false);
        assert_eq!(
            (off.ControlMask, off.StateMask),
            (THREAD_POWER_THROTTLING_EXECUTION_SPEED, 0)
        );
    }

    /// The real calls: Windows accepts both halves, and the thread reads back below normal and,
    /// where Windows can read the throttle back, under EcoQoS. On its own thread, so the harness
    /// thread keeps its priority for other tests.
    #[test]
    fn a_utility_thread_runs_below_normal_under_eco_qos() {
        let handle = std::thread::spawn(|| {
            let before = current_priority();
            let applied = apply_tier(QosClass::Utility);
            (before, applied, current_priority(), current_throttling())
        });
        let (before, applied, after, throttling) = handle.join().expect("qos test thread panicked");
        assert_eq!(before, THREAD_PRIORITY_NORMAL);
        assert_eq!(
            applied,
            Applied {
                priority: true,
                eco_qos: true
            }
        );
        assert_eq!(after, THREAD_PRIORITY_BELOW_NORMAL);
        match throttling {
            Ok(state) => {
                assert_eq!(state.ControlMask, THREAD_POWER_THROTTLING_EXECUTION_SPEED);
                assert_eq!(state.StateMask, THREAD_POWER_THROTTLING_EXECUTION_SPEED);
            }
            // Windows 10 sets the throttle but can't read it back: `GetThreadInformation` refuses
            // the class with `ERROR_INVALID_PARAMETER` (verified on Windows 10 22H2, build 19045,
            // mami, 2026-11-19). Windows 11 reads it back.
            Err(error) => assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_PARAMETER as i32), "{error}"),
        }
    }
}
