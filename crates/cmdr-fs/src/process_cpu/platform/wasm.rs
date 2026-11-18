//! The browser build has no threads or process times to read.

pub(crate) fn take_snapshot() -> Option<crate::process_cpu::CpuSnapshot> {
    None
}
