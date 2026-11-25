//! The single-threaded COM apartment every Shell call asks for, in its two shapes:
//!
//! - [`in_apartment`]: entered and left around ONE call, on a thread made for it (a launch, a
//!   reveal, the Open-with list). Typed failure: the call doesn't run outside an apartment.
//! - [`ensure_on_this_thread`]: entered on a pooled thread's first ask and left when the thread
//!   ends (the icon lookups, which run many calls per thread).
//!
//! Both pass `COINIT_DISABLE_OLE1DDE`, as Microsoft asks of every Shell caller. The Shell's
//! objects (`IShellItem`, `IShellLinkW`, the image factory, `IAssocHandler`) are documented for
//! STA, and an apartment-threaded third-party handler can fail quietly from an MTA.

use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};

/// `CoInitializeEx` for an STA on the calling thread: `S_OK`, `S_FALSE` (already in one), or a
/// failure `HRESULT`.
fn enter() -> i32 {
    // SAFETY: no reserved pointer; the callers balance a success (`S_OK` or `S_FALSE`) with
    // exactly one `CoUninitialize` on this same thread.
    unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).cast_unsigned(),
        )
    }
}

/// Leaves the apartment [`in_apartment`] entered, on drop, so a panicking call leaves it too.
struct ScopedApartment;

impl Drop for ScopedApartment {
    fn drop(&mut self) {
        // SAFETY: a `ScopedApartment` exists only after a successful `CoInitializeEx` on this
        // thread (it's never sent), and this balances it exactly once.
        unsafe { CoUninitialize() };
    }
}

/// Runs `call` inside a single-threaded apartment entered and left on THIS thread. Call it on a
/// fresh thread: one that already joined the multithreaded apartment can't enter this one.
///
/// # Errors
///
/// The `CoInitializeEx` `HRESULT` when the apartment can't be entered; `call` didn't run.
pub(crate) fn in_apartment<T>(call: impl FnOnce() -> T) -> Result<T, i32> {
    let entered = enter();
    if entered < 0 {
        return Err(entered);
    }
    let _apartment = ScopedApartment;
    Ok(call())
}

/// Keeps this thread's apartment serving its window messages for `bound`, then returns.
///
/// For a thread that just handed files to a handler through `IAssocHandler::Invoke` or the
/// "open with" dialog and is about to leave its apartment: there's no `SEE_MASK_NOASYNC` for
/// those, and a DDE or asynchronous handler finishes its launch through messages to this
/// apartment, so a thread that left at once could drop it. Microsoft's advice for a short-lived
/// thread is to stay and pump; `bound` caps how long.
pub(crate) fn pump_messages_for(bound: std::time::Duration) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage,
    };

    let until = std::time::Instant::now() + bound;
    loop {
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return;
        }
        let wait_ms = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: no handles to wait on (count 0, null array); wakes on any input for this
        // thread's queue or at the timeout.
        unsafe { MsgWaitForMultipleObjects(0, std::ptr::null(), 0, wait_ms, QS_ALLINPUT) };
        let mut msg = MSG::default();
        // SAFETY: `msg` is a writable `MSG`; a null window takes every message for this thread.
        while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: `msg` holds the message `PeekMessageW` just removed from this thread's queue.
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

/// This thread's membership, held for the thread's life.
struct ThreadApartment {
    entered: bool,
}

impl Drop for ThreadApartment {
    fn drop(&mut self) {
        if self.entered {
            // SAFETY: balances the successful `CoInitializeEx` in `ensure_on_this_thread`;
            // thread-local destructors run on the thread that built the value.
            unsafe { CoUninitialize() };
        }
    }
}

thread_local! {
    static THREAD_APARTMENT: ThreadApartment = {
        let hr = enter();
        // A thread another library already put in a multithreaded apartment fails with
        // `RPC_E_CHANGED_MODE` and keeps it: not ours to leave, and COM still runs an
        // apartment-threaded handler for it in a host apartment.
        ThreadApartment { entered: hr >= 0 }
    };
}

/// Makes sure this thread is in a COM apartment before it asks the Shell, entering an STA on the
/// first ask and leaving it when the thread ends.
pub(crate) fn ensure_on_this_thread() {
    THREAD_APARTMENT.with(|_| ());
}
