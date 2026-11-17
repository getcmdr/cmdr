//! What both Unixes share: the SIGBUS entry point, the one `mmap` that maps zeros over a page,
//! and the test process's handler. Each arm answers only which page, from its own region reader.

use std::sync::atomic::Ordering;

use super::super::GENERATION;

/// Called first by a SIGBUS handler. Maps zeros over the faulting page and answers `true` when
/// the fault is a read of a read-only, file-backed page outside every loaded image; then the
/// handler must return, so the load re-executes. Answers `false`, touching nothing, for every
/// other fault, which the handler then treats as the crash it is.
///
/// # Safety
///
/// Call it only from a signal handler for `sig`, with the `siginfo_t` the kernel passed that
/// handler. It's async-signal-safe (see the module docs).
pub unsafe fn recover(sig: libc::c_int, info: *const libc::siginfo_t) -> bool {
    if sig != libc::SIGBUS || info.is_null() {
        return false;
    }
    // SAFETY: the caller passes the kernel's `siginfo_t` for this SIGBUS, which is live for the
    // handler's duration.
    let recovered = super::imp::recover_page(unsafe { &*info });
    if recovered {
        GENERATION.fetch_add(1, Ordering::SeqCst);
    }
    recovered
}

/// Maps one page of anonymous, read-only zeros over `page`.
///
/// `MAP_FIXED` replaces exactly that page of the dead file mapping and nothing else, so its owner
/// can later `munmap` the whole range as it always would.
pub(crate) fn map_zeros_over(page: u64, page_size: u64) -> bool {
    // SAFETY: `page` is page-aligned and lies inside a file-backed, read-only, non-image region
    // the caller just looked up, so replacing it changes only memory whose next read would
    // otherwise kill the process. `mmap` is a raw syscall: async-signal-safe in practice.
    let mapped = unsafe {
        libc::mmap(
            page as *mut libc::c_void,
            page_size as libc::size_t,
            libc::PROT_READ,
            libc::MAP_FIXED | libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    mapped != libc::MAP_FAILED
}

/// A SIGBUS handler for a test process that has no crash reporter: it calls [`recover`] and, for
/// a fault it can't own, restores the default action and returns, so the load re-executes and the
/// process dies of the signal as it would have.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn install_test_handler() {
    extern "C" fn on_bus(sig: libc::c_int, info: *mut libc::siginfo_t, _context: *mut libc::c_void) {
        // SAFETY: the kernel hands this handler its own `siginfo_t`.
        if unsafe { recover(sig, info) } {
            return;
        }
        // SAFETY: `signal` is async-signal-safe; restoring the default makes the re-executed load
        // fatal.
        unsafe { libc::signal(sig, libc::SIG_DFL) };
    }
    super::imp::prepare();
    // SAFETY: `action` starts zeroed, `sa_mask` is emptied, and `sa_sigaction` is a valid
    // `SA_SIGINFO` handler with the matching signature.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_bus as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGBUS, &action, std::ptr::null_mut());
    }
}
