//! No reader on Windows yet. Its census is a `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD)`
//! walk filtered to this process, then `OpenThread`, `GetThreadTimes`, and
//! `GetThreadDescription` per thread (each a handle to close and a wide string to free), with
//! `GetProcessTimes` for the total. The `windows-sys` dependency is here (`thread_cpu` reads
//! `GetThreadTimes` through it); the census itself isn't written, and task 13 of
//! `docs/specs/windows-port-plan.md` owns it. Until then `cpu_diagnostics` isn't offered
//! there and the transfer stall log carries no CPU line.

pub(crate) fn take_snapshot() -> Option<crate::process_cpu::CpuSnapshot> {
    None
}
