//! macOS: PAC files run by CFNetwork itself (`CFNetworkExecuteProxyAutoConfigurationURL` /
//! `…Script`). Auto proxy discovery (WPAD) arrives as an ordinary PAC entry: with it on, macOS
//! hands over the discovered PAC URL (`http://wpad/wpad.dat` by DNS).
//!
//! The evaluation is asynchronous on a run loop, so [`execute`] runs it on the calling thread's
//! own run loop (the short-lived thread `crate::pac` starts for each miss).

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFArray, CFError, CFIndex, CFRetained, CFRunLoop, CFRunLoopSource, CFString, CFURL};

use crate::entries::cf::{cf_url, entries_from};
use crate::entries::{Entry, PacSource};

/// What the callback leaves for [`execute`]: `None` until it fires, then the answer or a failure.
type Slot = RefCell<Option<Option<Vec<Entry>>>>;

/// Runs the PAC on THIS thread's run loop, in a private mode so nothing else on it runs, until
/// the callback fires or `timeout` passes. `AutoDetect` has no runner here: macOS hands WPAD's
/// answer over as a PAC URL, so that source never comes from a Mac's settings.
pub(crate) fn execute(source: &PacSource, url: &str, timeout: Duration) -> Option<Vec<Entry>> {
    let target = cf_url(url)?;
    let slot: Slot = RefCell::new(None);
    let mut context = StreamClientContext {
        version: 0,
        info: (&raw const slot).cast_mut().cast(),
        retain: None,
        release: None,
        copy_description: None,
    };
    let raw = match source {
        PacSource::Url(pac_url) => {
            let pac_url = cf_url(pac_url)?;
            // SAFETY: both URLs are valid CFURLs for the call; `context` outlives the call (CF copies
            // it), and its `info` points at `slot`, which outlives the run-loop source: the source is
            // invalidated below before `slot` goes out of scope, so the callback can't fire later.
            unsafe { CFNetworkExecuteProxyAutoConfigurationURL(&*pac_url, &*target, on_result, &mut context) }
        }
        PacSource::Script(script) => {
            let script = CFString::from_str(script);
            // SAFETY: as for the URL variant above, with a valid CFString script.
            unsafe { CFNetworkExecuteProxyAutoConfigurationScript(&*script, &*target, on_result, &mut context) }
        }
        PacSource::AutoDetect => return None,
    };
    // SAFETY: `raw` comes from a CFNetwork `Execute…` call, which returns a +1 reference the
    // caller owns ("Create rule"), or null.
    let run_loop_source = unsafe { CFRetained::from_raw(NonNull::new(raw)?) };
    let deadline = Instant::now() + timeout;
    run_source_until(&run_loop_source, &slot, deadline);
    run_loop_source.invalidate();
    slot.into_inner().flatten()
}

/// Runs `source` on this thread's run loop until `slot` is filled or `deadline` passes.
fn run_source_until(source: &CFRunLoopSource, slot: &Slot, deadline: Instant) {
    let Some(run_loop) = CFRunLoop::current() else { return };
    let mode = CFString::from_static_str("com.veszelovszki.cmdr.pac");
    run_loop.add_source(Some(source), Some(&mode));
    while slot.borrow().is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        CFRunLoop::run_in_mode(Some(&mode), left.as_secs_f64(), true);
    }
}

/// CFNetwork's result callback. `client` is the `info` from [`execute`]'s context.
extern "C" fn on_result(client: *mut c_void, proxies: *const CFArray, error: *const CFError) {
    // SAFETY: `client` is `&slot` from `execute`, on this same thread's stack: the callback only
    // fires inside `run_source_until` on that thread, before the source is invalidated.
    let slot = unsafe { &*client.cast_const().cast::<Slot>() };
    let answer = if error.is_null() && !proxies.is_null() {
        // SAFETY: non-null, and CFNetwork keeps the list alive for the callback's duration.
        Some(entries_from(unsafe { &*proxies }))
    } else {
        if !error.is_null() {
            // SAFETY: non-null, and CFNetwork keeps the error alive for the callback's duration.
            let error = unsafe { &*error };
            log::debug!(target: "cmdr_http", "PAC evaluation refused: code {}", error.code());
        }
        None
    };
    *slot.borrow_mut() = Some(answer);
}

/// `CFStreamClientContext`, laid out as CFNetwork reads it. No retain or release: `info` is a
/// stack slot whose lifetime [`execute`] guarantees.
#[repr(C)]
struct StreamClientContext {
    version: CFIndex,
    info: *mut c_void,
    retain: Option<extern "C" fn(*mut c_void) -> *mut c_void>,
    release: Option<extern "C" fn(*mut c_void)>,
    copy_description: Option<extern "C" fn(*mut c_void) -> *const CFString>,
}

type ResultCallback = extern "C" fn(*mut c_void, *const CFArray, *const CFError);

// Declared here rather than taken from `objc2-cf-network`: its binding types the callback's
// proxy list as non-null, and CFNetwork passes null when the PAC fails.
#[link(name = "CFNetwork", kind = "framework")]
unsafe extern "C" {
    fn CFNetworkExecuteProxyAutoConfigurationURL(
        proxy_auto_config_url: *const CFURL,
        target_url: *const CFURL,
        callback: ResultCallback,
        client_context: *mut StreamClientContext,
    ) -> *mut CFRunLoopSource;
    fn CFNetworkExecuteProxyAutoConfigurationScript(
        proxy_auto_configuration_script: *const CFString,
        target_url: *const CFURL,
        callback: ResultCallback,
        client_context: *mut StreamClientContext,
    ) -> *mut CFRunLoopSource;
}

#[cfg(test)]
#[path = "macos_test.rs"]
mod tests;
