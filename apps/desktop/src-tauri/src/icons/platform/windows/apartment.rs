//! One single-threaded COM apartment per thread that asks the Shell, entered on the first ask
//! and left when the thread ends. The Shell's image factory and `ShellLink` are documented for
//! STA, and an apartment-threaded third-party icon handler can fail quietly from an MTA.

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize};

/// This thread's membership, held for the thread's life.
struct Apartment {
    entered: bool,
}

impl Apartment {
    /// Enters an STA (`S_FALSE`, already in one, counts and is balanced the same). A thread
    /// another library already put in a multithreaded apartment (`RPC_E_CHANGED_MODE`) keeps
    /// it: not ours to leave, and COM still runs an apartment-threaded handler for it in a host
    /// apartment.
    fn enter() -> Self {
        // SAFETY: initializing COM on the calling thread; a success is balanced by `Drop`,
        // which runs on this same thread when its thread-local storage is torn down.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Self {
            entered: hr.is_ok() && hr != RPC_E_CHANGED_MODE,
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.entered {
            // SAFETY: balances the successful `CoInitializeEx` in `enter`; thread-local
            // destructors run on the thread that built the value.
            unsafe { CoUninitialize() };
        }
    }
}

thread_local! {
    static APARTMENT: Apartment = Apartment::enter();
}

/// Makes sure this thread is in a COM apartment before it asks the Shell.
pub(super) fn ensure() {
    APARTMENT.with(|_| ());
}
