//! Windows: the unhandled-exception filter and the CRT's `SIGABRT` handler
//! (`exception_filter.rs`); the image base and thread count from Win32.

use std::path::Path;

use super::exception_filter;

/// The exception filter and the abort handler, writing a record to `raw_crash_path`.
pub(crate) fn install_native_crash_hooks(raw_crash_path: &Path, _data_dir: &Path) {
    exception_filter::install(raw_crash_path);
}

/// `crash-report.<pid>-<start ms>.raw` beside the fixed name. Windows lets a second instance start
/// (nothing guards it), and with one shared file the second's next-launch pass would delete the
/// first's live record and its install would replace it.
pub(crate) fn session_raw_crash_path(fixed: &Path) -> std::path::PathBuf {
    let started_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    fixed.with_file_name(format!("crash-report.{}-{started_ms}.raw", std::process::id()))
}

/// The owning process still holds the file for writing, so an open that refuses to share writing
/// fails with a sharing violation. When the owner ends, the OS closes its handle and the open goes
/// through. An error code, never a message.
pub(crate) fn raw_crash_file_in_use(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    match std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
    {
        Ok(_) => false,
        Err(error) => error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32),
    }
}

/// An access violation in `cmdr.exe`, in the exception record's format.
#[cfg(test)]
pub(crate) fn sample_raw_crash() -> Vec<u8> {
    use super::exception_record::{Header, Kind, write_frame, write_header, write_module};
    let mut bytes = Vec::new();
    let header = Header {
        kind: Kind::Exception,
        code: 0xC000_0005,
        address: 0x7ff6_1000_1a2b,
        param_count: 2,
        params: [0, 0x10, 0],
        image_base: 0x7ff6_1000_0000,
        uptime_ms: 1_000,
    };
    write_header(&mut bytes, &header, "0.1.0");
    write_module(
        &mut bytes,
        0x7ff6_1000_0000,
        &"cmdr.exe".encode_utf16().collect::<Vec<_>>(),
    );
    write_frame(&mut bytes, 0x7ff6_1000_1a2b, 0x7ff6_1000_0000);
    bytes
}

/// The exception filter's record, through `next_launch`.
pub(crate) fn convert_raw_crash(crash_json_path: &Path, raw_crash_path: &Path) {
    super::super::next_launch::convert_raw_exception_crash(crash_json_path, raw_crash_path);
}

/// `GetModuleHandleW(NULL)`: the executable's own load address.
pub(crate) fn current_image_base() -> u64 {
    exception_filter::current_image_base()
}

/// A Toolhelp snapshot's threads that belong to this process, or 0 when the snapshot fails.
pub(crate) fn current_thread_count() -> usize {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };

    let pid = std::process::id();
    // SAFETY: a thread snapshot takes no process id (0 is ignored for `TH32CS_SNAPTHREAD`); the
    // handle we get back is closed below on every path.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return 0;
    }
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut count = 0;
    // SAFETY: `snapshot` is the open snapshot and `entry` a stack local whose `dwSize` is set, as
    // both calls require; they only write `entry`.
    let mut more = unsafe { Thread32First(snapshot, &raw mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            count += 1;
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: the snapshot handle we opened, closed exactly once.
    unsafe { CloseHandle(snapshot) };
    count
}

/// The panic hook's fallback stack: `module+0xoffset` frames, which resolve against the build's
/// PDB off the machine when the shipped binary can't name its own frames.
pub(crate) fn module_backtrace_frames() -> Option<Vec<String>> {
    Some(exception_filter::module_frames())
}
