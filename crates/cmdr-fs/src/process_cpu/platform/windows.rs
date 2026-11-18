//! No reader on Windows yet. Its census is a `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD)`
//! walk with `GetThreadTimes` and `GetThreadDescription` per thread and `GetProcessTimes`
//! for the total, which needs a Windows API dependency this crate doesn't carry; task 13
//! of `docs/specs/windows-port-plan.md` owns it. Until then `cpu_diagnostics` isn't offered
//! there and the transfer stall log carries no CPU line.

pub(crate) fn take_snapshot() -> Option<crate::process_cpu::CpuSnapshot> {
    None
}
