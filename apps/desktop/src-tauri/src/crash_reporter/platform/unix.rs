use std::path::Path;

use super::super::signal_handler;

/// The async-signal-safe handlers for the fatal signals, writing raw addresses to a pre-opened
/// fd at `raw_crash_path`.
pub(crate) fn install_signal_handlers(raw_crash_path: &Path) {
    signal_handler::install(raw_crash_path);
}

/// The fixed path: one instance writes at a time.
pub(crate) fn session_raw_crash_path(fixed: &Path) -> std::path::PathBuf {
    fixed.to_path_buf()
}

/// Never: only one instance writes, and it opens its file after the next-launch pass.
pub(crate) fn raw_crash_file_in_use(_path: &Path) -> bool {
    false
}

/// A SIGSEGV with two frames, in the signal handler's v2 format.
#[cfg(test)]
pub(crate) fn sample_raw_crash() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"CMCR");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&libc::SIGSEGV.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0x1_0000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&0x1_0000_1000u64.to_le_bytes());
    bytes.extend_from_slice(&0x1_0000_2000u64.to_le_bytes());
    let mut version = [0u8; 32];
    version[..5].copy_from_slice(b"0.1.0");
    bytes.extend_from_slice(&version);
    bytes
}

/// The signal handler's raw file, through `next_launch`.
pub(crate) fn convert_raw_crash(crash_json_path: &Path, raw_crash_path: &Path) {
    super::super::next_launch::convert_raw_signal_crash(crash_json_path, raw_crash_path);
}

/// `None`: a Unix binary carries its symbol table, so the panic hook's own backtrace names our
/// frames.
pub(crate) fn module_backtrace_frames() -> Option<Vec<String>> {
    None
}

/// Read through `dladdr` on the main image (`signal_handler.rs`).
pub(crate) fn current_image_base() -> u64 {
    signal_handler::current_image_base()
}
