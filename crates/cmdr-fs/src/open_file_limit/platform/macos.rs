//! macOS: the shared POSIX raise (`unix.rs`), held to `kern.maxfilesperproc`. `setrlimit` refuses
//! `RLIM_INFINITY` or any soft value above that ceiling (`EINVAL`, the `OPEN_MAX` semantics in
//! `setrlimit(2)`'s compatibility notes), and the hard limit is usually `RLIM_INFINITY`, so this
//! clamp is what lets the raise land at all.

use super::super::OpenFileLimit;

pub(crate) fn raise() -> OpenFileLimit {
    super::unix::raise_within(max_files_per_proc())
}

/// `kern.maxfilesperproc`. `None` when the read fails, which leaves the hard limit and the target
/// to clamp.
fn max_files_per_proc() -> Option<u64> {
    let mut value: libc::c_int = 0;
    let mut size = size_of::<libc::c_int>();
    // SAFETY: the name is a NUL-terminated C string, and `value` / `size` describe a writable
    // `c_int` the call fills in; the null new-value pointer with length 0 asks for a read only.
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.maxfilesperproc".as_ptr(),
            (&raw mut value).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    u64::try_from(value).ok()
}
