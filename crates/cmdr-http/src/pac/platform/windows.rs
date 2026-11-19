//! Windows: PAC files run by WinHTTP's asynchronous resolver (`WinHttpGetProxyForUrlEx`), and
//! WPAD's discovery (`WinHttpDetectAutoProxyConfigUrl`), which the system arm runs in the
//! background (`system/platform/windows.rs`). On Windows 8 and later both happen in the WinHTTP
//! Web Proxy Auto-Discovery service, which also caches the fetched script; Cmdr's own cache
//! (`crate::pac`) sits on top, so a miss there is one call here.
//!
//! Why the resolver, not the older synchronous `WinHttpGetProxyForUrl` (both verified on Windows
//! 10 22H2, mami, a loopback PAC server, 2026-11-19):
//!
//! - It answers typed entries: `SOCKS` (scheme 4), `HTTPS` (scheme 2), and a trailing `DIRECT`.
//!   The older call turned any PAC answer naming SOCKS into "no proxy", which would send Cmdr
//!   direct where the PAC said to tunnel, and dropped the trailing `DIRECT`.
//! - Closing the resolver cancels a running evaluation at once (`ERROR_WINHTTP_OPERATION_CANCELLED`,
//!   then `HANDLE_CLOSING` as its last callback), so a PAC server that accepts and never answers
//!   costs the evaluation thread its budget and no more.
//!
//! The resolver answers through a status callback on WinHTTP's own threads; [`CALLS`] hands each
//! answer to the thread waiting for it, keyed by the resolver handle.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_IO_PENDING, GetLastError, GlobalFree};
use windows_sys::Win32::Networking::WinHttp::{
    ERROR_WINHTTP_LOGIN_FAILURE, ERROR_WINHTTP_TIMEOUT, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_ASYNC_RESULT,
    WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A, WINHTTP_AUTOPROXY_CONFIG_URL,
    WINHTTP_AUTOPROXY_OPTIONS, WINHTTP_CALLBACK_FLAG_GETPROXYFORURL_COMPLETE, WINHTTP_CALLBACK_FLAG_REQUEST_ERROR,
    WINHTTP_CALLBACK_STATUS_GETPROXYFORURL_COMPLETE, WINHTTP_CALLBACK_STATUS_REQUEST_ERROR, WINHTTP_FLAG_ASYNC,
    WINHTTP_INTERNET_SCHEME_HTTP, WINHTTP_INTERNET_SCHEME_HTTPS, WINHTTP_INTERNET_SCHEME_SOCKS, WINHTTP_PROXY_RESULT,
    WINHTTP_PROXY_RESULT_ENTRY, WinHttpCloseHandle, WinHttpCreateProxyResolver, WinHttpDetectAutoProxyConfigUrl,
    WinHttpFreeProxyResult, WinHttpGetProxyForUrlEx, WinHttpGetProxyResult, WinHttpOpen, WinHttpSetStatusCallback,
    WinHttpSetTimeouts,
};
use windows_sys::core::PWSTR;

use crate::entries::{Entry, PacSource};
use crate::system::platform::ie_settings::{PacScheme, pac_proxy_entry};

/// Runs the PAC `source` names for `url`. An inline script has no runner: Windows' settings name
/// a PAC only by URL, so that source never comes from them.
pub(crate) fn execute(source: &PacSource, url: &str, timeout: Duration) -> Option<Vec<Entry>> {
    let PacSource::Url(pac_url) = source else {
        return None;
    };
    let session = Session::open(timeout)?;
    let pac_url = wide(pac_url);
    let target = wide(url);
    let mut options = WINHTTP_AUTOPROXY_OPTIONS {
        dwFlags: WINHTTP_AUTOPROXY_CONFIG_URL,
        dwAutoDetectFlags: 0,
        lpszAutoConfigUrl: pac_url.as_ptr(),
        lpvReserved: std::ptr::null_mut(),
        dwReserved: 0,
        // First without the Windows sign-in, as Microsoft advises: only a PAC server that asks
        // for it (`ERROR_WINHTTP_LOGIN_FAILURE`) gets it, on the retry below.
        fAutoLogonIfChallenged: 0,
    };
    let answer = session.resolve(&target, &options, timeout).or_else(|code| {
        if code != ERROR_WINHTTP_LOGIN_FAILURE {
            return Err(code);
        }
        options.fAutoLogonIfChallenged = 1;
        session.resolve(&target, &options, timeout)
    });
    match answer {
        Ok(entries) => Some(entries),
        Err(code) => {
            log::debug!(target: "cmdr_http", "WinHTTP couldn't run the PAC: error {code}");
            None
        }
    }
}

/// WPAD's PAC URL on this network (DHCP, then DNS), or `None` when there's none. Blocks for as
/// long as discovery takes, so only the system arm's background discovery calls it.
pub(crate) fn detect_pac_url() -> Option<String> {
    let mut found: PWSTR = std::ptr::null_mut();
    // SAFETY: `found` is a live out-pointer; on success it holds a `GlobalAlloc`ed string the
    // caller frees, which `take_global_string` does.
    let ok = unsafe {
        WinHttpDetectAutoProxyConfigUrl(
            WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A,
            &mut found,
        )
    };
    if ok == 0 {
        // SAFETY: no arguments; reads this thread's last error, set by the call just made.
        let code = unsafe { GetLastError() };
        log::debug!(target: "cmdr_http", "no WPAD on this network: error {code}");
        return None;
    }
    // SAFETY: the call succeeded, so `found` is null or the string it allocated for us.
    unsafe { take_global_string(found) }
}

/// How a resolver's evaluation ended, as its callback reports it.
enum Outcome {
    Complete,
    Failed(u32),
}

/// The waiting thread for each running resolver, by handle. A callback for a handle that isn't
/// here (one already given up on and closed) is dropped.
static CALLS: LazyLock<Mutex<HashMap<usize, mpsc::Sender<Outcome>>>> = LazyLock::new(Default::default);

/// WinHTTP's status callback, on WinHTTP's threads. ❌ Must never panic (it's called across FFI).
unsafe extern "system" fn on_status(handle: *mut c_void, _context: usize, status: u32, info: *mut c_void, _: u32) {
    let outcome = match status {
        WINHTTP_CALLBACK_STATUS_GETPROXYFORURL_COMPLETE => Outcome::Complete,
        WINHTTP_CALLBACK_STATUS_REQUEST_ERROR => {
            let result = info.cast::<WINHTTP_ASYNC_RESULT>();
            if result.is_null() {
                Outcome::Failed(0)
            } else {
                // SAFETY: for `REQUEST_ERROR`, WinHTTP passes a `WINHTTP_ASYNC_RESULT` it keeps
                // alive for the callback's duration; null was handled above.
                let error = unsafe { (*result).dwError };
                Outcome::Failed(error)
            }
        }
        _ => return,
    };
    let calls = CALLS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(waiting) = calls.get(&(handle as usize)) {
        // The waiter may have given up already; nothing else to tell.
        let _ = waiting.send(outcome);
    }
}

/// An asynchronous WinHTTP session that talks to the PAC server directly, closed on drop.
struct Session(*mut c_void);

impl Session {
    fn open(timeout: Duration) -> Option<Self> {
        let agent = wide("Cmdr");
        // SAFETY: `agent` is a NUL-terminated UTF-16 string that outlives the call; the access
        // type takes no proxy name or bypass list, so both are null.
        let handle = unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                WINHTTP_FLAG_ASYNC,
            )
        };
        if handle.is_null() {
            return None;
        }
        let session = Session(handle);
        let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: `session.0` is the live session handle just opened, the four timeouts are
        // non-negative milliseconds, and `on_status` is a `'static` function that never unwinds.
        // A refusal of either keeps WinHTTP's defaults or leaves no callback; the evaluation's
        // own wait bounds both.
        unsafe {
            let _ = WinHttpSetTimeouts(session.0, millis, millis, millis, millis);
            let _ = WinHttpSetStatusCallback(
                session.0,
                Some(on_status),
                WINHTTP_CALLBACK_FLAG_GETPROXYFORURL_COMPLETE | WINHTTP_CALLBACK_FLAG_REQUEST_ERROR,
                0,
            );
        }
        Some(session)
    }

    /// One evaluation on a fresh resolver: the PAC's list, or the Win32 error code. Gives up
    /// after `timeout`, cancelling the evaluation by closing its resolver.
    fn resolve(
        &self,
        target: &[u16],
        options: &WINHTTP_AUTOPROXY_OPTIONS,
        timeout: Duration,
    ) -> Result<Vec<Entry>, u32> {
        let resolver = Resolver::create(self)?;
        // SAFETY: `resolver.handle` is live; `target` and the PAC URL `options` points at are
        // NUL-terminated UTF-16 that outlive the evaluation (it ends, or is cancelled by the
        // resolver's close, before this function returns and drops them).
        let started = unsafe { WinHttpGetProxyForUrlEx(resolver.handle, target.as_ptr(), options, 0) };
        if started != ERROR_IO_PENDING {
            return Err(started);
        }
        match resolver.outcome.recv_timeout(timeout) {
            Ok(Outcome::Complete) => resolver.entries(),
            Ok(Outcome::Failed(code)) => Err(code),
            Err(_) => Err(ERROR_WINHTTP_TIMEOUT),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the session handle `open` got, closed exactly once, here, after
        // every resolver made from it (each borrows the session).
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

/// One proxy resolver and the channel its callback answers on; closing it (on drop) cancels an
/// evaluation still running.
struct Resolver<'a> {
    handle: *mut c_void,
    outcome: mpsc::Receiver<Outcome>,
    _session: &'a Session,
}

impl<'a> Resolver<'a> {
    fn create(session: &'a Session) -> Result<Self, u32> {
        let mut handle = std::ptr::null_mut();
        // SAFETY: `session.0` is a live asynchronous session; `handle` is a live out-pointer.
        let code = unsafe { WinHttpCreateProxyResolver(session.0, &mut handle) };
        if code != 0 {
            return Err(code);
        }
        let (sender, outcome) = mpsc::channel();
        CALLS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(handle as usize, sender);
        Ok(Resolver {
            handle,
            outcome,
            _session: session,
        })
    }

    /// The finished evaluation's list.
    fn entries(&self) -> Result<Vec<Entry>, u32> {
        let mut result = WINHTTP_PROXY_RESULT::default();
        // SAFETY: the resolver's evaluation completed; `result` is a live, zeroed struct the call
        // fills with entries WinHTTP owns until `WinHttpFreeProxyResult` below.
        let code = unsafe { WinHttpGetProxyResult(self.handle, &mut result) };
        if code != 0 {
            return Err(code);
        }
        let entries = if result.pEntries.is_null() {
            Vec::new()
        } else {
            // SAFETY: WinHTTP hands over `cEntries` initialized entries at `pEntries`, alive
            // until the free below, and each `pwszProxy` is null or NUL-terminated UTF-16.
            unsafe { std::slice::from_raw_parts(result.pEntries, result.cEntries as usize) }
                .iter()
                .map(|entry| unsafe { entry_from(entry) })
                .collect()
        };
        // SAFETY: `result` was filled by `WinHttpGetProxyResult` and is freed exactly once, here,
        // after the last read of its strings.
        unsafe { WinHttpFreeProxyResult(&mut result) };
        Ok(entries)
    }
}

impl Drop for Resolver<'_> {
    fn drop(&mut self) {
        // Unhooked before the close, so a late callback (the cancellation's) finds nobody.
        CALLS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&(self.handle as usize));
        // SAFETY: `self.handle` is the resolver `create` got, closed exactly once, here.
        let _ = unsafe { WinHttpCloseHandle(self.handle) };
    }
}

/// One of the resolver's entries as an [`Entry`].
///
/// # Safety
///
/// `entry.pwszProxy` is null or a NUL-terminated UTF-16 string alive for the call.
unsafe fn entry_from(entry: &WINHTTP_PROXY_RESULT_ENTRY) -> Entry {
    if entry.fProxy == 0 {
        return Entry::Direct;
    }
    // SAFETY: the caller promises a NUL-terminated string or null.
    let host = unsafe { read_wide(entry.pwszProxy) }.unwrap_or_default();
    let scheme = match entry.ProxyScheme {
        WINHTTP_INTERNET_SCHEME_HTTP => PacScheme::Http,
        WINHTTP_INTERNET_SCHEME_HTTPS => PacScheme::Https,
        WINHTTP_INTERNET_SCHEME_SOCKS => PacScheme::Socks,
        other => PacScheme::Other(other),
    };
    pac_proxy_entry(scheme, &host, entry.ProxyPort)
}

/// `text` as NUL-terminated UTF-16.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads a NUL-terminated UTF-16 string someone else owns.
///
/// # Safety
///
/// `ptr` is null, or a NUL-terminated UTF-16 string alive for the call.
unsafe fn read_wide(ptr: PWSTR) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller promises a NUL-terminated string, so the scan stops inside it.
    let len = (0..).take_while(|&i| unsafe { *ptr.add(i) } != 0).count();
    // SAFETY: `len` units were just read as in bounds.
    Some(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(ptr, len)
    }))
}

/// Reads and frees a `GlobalAlloc`ed UTF-16 string WinHTTP handed over.
///
/// # Safety
///
/// `ptr` is null, or a NUL-terminated UTF-16 string from `GlobalAlloc` that nothing else frees.
pub(crate) unsafe fn take_global_string(ptr: PWSTR) -> Option<String> {
    // SAFETY: the caller's promise covers the read.
    let text = unsafe { read_wide(ptr) }?;
    // SAFETY: the caller promises `ptr` came from `GlobalAlloc` and is freed only here.
    unsafe { GlobalFree(ptr.cast()) };
    Some(text)
}

#[cfg(test)]
#[path = "windows_test.rs"]
mod tests;
