//! Windows: which network the machine is on, as a number that moves whenever an IP address comes
//! or goes (`NotifyUnicastIpAddressChange`): joining or leaving a network, a new DHCP lease, a VPN
//! coming up. Subscribed once, on the first ask, for the life of the process; Windows calls back,
//! nothing polls. What was worked out for one network (a PAC's answers, WPAD's discovery) is keyed
//! on this number, so a laptop that moves from the office to home never reuses the office's proxy.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use windows_sys::Win32::Foundation::{HANDLE, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    MIB_NOTIFICATION_TYPE, MIB_UNICASTIPADDRESS_ROW, NotifyUnicastIpAddressChange,
};
use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;

static GENERATION: AtomicU64 = AtomicU64::new(0);
static SUBSCRIBED: OnceLock<bool> = OnceLock::new();

/// The current network's number; subscribes to the change notification on the first call.
pub(crate) fn network_generation() -> u64 {
    SUBSCRIBED.get_or_init(|| subscribe(false));
    GENERATION.load(Ordering::SeqCst)
}

/// Asks Windows to call [`on_address_change`] on every address change, for the rest of the
/// process (the registration is never cancelled). `initial` also calls it once right away, which
/// only the test wants. `false` when Windows refused: then the number never moves, and the caches
/// fall back on their TTLs alone.
fn subscribe(initial: bool) -> bool {
    let mut registration: HANDLE = std::ptr::null_mut();
    // SAFETY: `on_address_change` is a `'static` function that never unwinds and reads nothing
    // through its arguments; no caller context; `registration` is a live out-pointer.
    let code = unsafe {
        NotifyUnicastIpAddressChange(
            AF_UNSPEC,
            Some(on_address_change),
            std::ptr::null(),
            initial,
            &mut registration,
        )
    };
    if code != NO_ERROR {
        log::warn!(target: "cmdr_http", "couldn't follow network changes (error {code}); PAC answers stay up to their TTL");
        return false;
    }
    true
}

/// Windows' callback, on its own threads. ❌ Must never panic (it's called across FFI).
unsafe extern "system" fn on_address_change(
    _context: *const c_void,
    _row: *const MIB_UNICASTIPADDRESS_ROW,
    _change: MIB_NOTIFICATION_TYPE,
) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// The registration works and Windows' calls reach the number. A real network change can't be
    /// made on a shared builder, so the subscription's own first call stands in for one.
    #[test]
    fn windows_change_notification_moves_the_network_number() {
        let before = network_generation();
        assert!(subscribe(true), "NotifyUnicastIpAddressChange refused");
        let deadline = Instant::now() + Duration::from_secs(10);
        while GENERATION.load(Ordering::SeqCst) == before {
            assert!(Instant::now() < deadline, "no callback within 10 s");
            std::thread::yield_now();
        }
        assert!(network_generation() > before);
    }
}
