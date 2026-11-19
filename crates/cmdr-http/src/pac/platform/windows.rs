//! Windows: PAC files and WPAD, run by WinHTTP (`WinHttpGetProxyForUrl`). On Windows 8 and later
//! the evaluation happens in the WinHTTP Web Proxy Auto-Discovery service, which also caches the
//! fetched script and WPAD's discovery; Cmdr's own cache (`crate::pac`) sits on top, so a miss
//! there is one call here.
//!
//! The call is synchronous and can block for seconds (DHCP and DNS discovery, fetching the file),
//! so it runs on the short-lived thread `crate::pac` starts for each miss, and `timeout` bounds
//! the script's download through the session's own timeouts.

use std::time::Duration;

use windows_sys::Win32::Foundation::{GetLastError, GlobalFree};
use windows_sys::Win32::Networking::WinHttp::{
    ERROR_WINHTTP_LOGIN_FAILURE, WINHTTP_ACCESS_TYPE_NAMED_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY,
    WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A, WINHTTP_AUTOPROXY_AUTO_DETECT,
    WINHTTP_AUTOPROXY_CONFIG_URL, WINHTTP_AUTOPROXY_OPTIONS, WINHTTP_PROXY_INFO, WinHttpCloseHandle,
    WinHttpGetProxyForUrl, WinHttpOpen, WinHttpSetTimeouts,
};
use windows_sys::core::PWSTR;

use crate::entries::{Entry, PacSource};
use crate::system::platform::ie_settings::proxy_list_entries;

/// Runs the PAC `source` names for `url`. An inline script has no runner: Windows' settings name
/// a PAC only by URL, so that source never comes from them.
pub(crate) fn execute(source: &PacSource, url: &str, timeout: Duration) -> Option<Vec<Entry>> {
    let pac_url = match source {
        PacSource::Url(pac_url) => Some(wide(pac_url)),
        PacSource::AutoDetect => None,
        PacSource::Script(_) => return None,
    };
    let session = Session::open(timeout)?;
    let target = wide(url);
    let mut options = WINHTTP_AUTOPROXY_OPTIONS {
        dwFlags: if pac_url.is_some() {
            WINHTTP_AUTOPROXY_CONFIG_URL
        } else {
            WINHTTP_AUTOPROXY_AUTO_DETECT
        },
        dwAutoDetectFlags: if pac_url.is_some() {
            0
        } else {
            WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A
        },
        lpszAutoConfigUrl: pac_url.as_ref().map_or(std::ptr::null(), |pac| pac.as_ptr()),
        lpvReserved: std::ptr::null_mut(),
        dwReserved: 0,
        // First without the Windows sign-in, as Microsoft advises: only a PAC server that asks
        // for it (`ERROR_WINHTTP_LOGIN_FAILURE`) gets it, on the retry below.
        fAutoLogonIfChallenged: 0,
    };
    let answer = session.proxy_for(&target, &mut options).or_else(|code| {
        if code != ERROR_WINHTTP_LOGIN_FAILURE {
            return Err(code);
        }
        options.fAutoLogonIfChallenged = 1;
        session.proxy_for(&target, &mut options)
    });
    match answer {
        Ok(entries) => Some(entries),
        Err(code) => {
            log::debug!(target: "cmdr_http", "WinHTTP couldn't run the PAC: error {code}");
            None
        }
    }
}

/// A WinHTTP session that talks to the PAC server directly, closed on drop.
struct Session(*mut core::ffi::c_void);

impl Session {
    fn open(timeout: Duration) -> Option<Self> {
        let agent = wide("Cmdr");
        // SAFETY: `agent` is a NUL-terminated UTF-16 string that outlives the call; the access
        // type takes no proxy name or bypass list, so both are null; no flags (synchronous).
        let handle = unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        if handle.is_null() {
            return None;
        }
        let session = Session(handle);
        let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: `session.0` is the live session handle just opened; the four timeouts are
        // non-negative milliseconds. A refusal keeps WinHTTP's defaults, which `crate::pac`'s
        // wait bounds anyway.
        let _ = unsafe { WinHttpSetTimeouts(session.0, millis, millis, millis, millis) };
        Some(session)
    }

    /// One `WinHttpGetProxyForUrl` call: the PAC's list, or the Win32 error code.
    fn proxy_for(&self, target: &[u16], options: &mut WINHTTP_AUTOPROXY_OPTIONS) -> Result<Vec<Entry>, u32> {
        let mut info = WINHTTP_PROXY_INFO::default();
        // SAFETY: `self.0` is a live session; `target` is NUL-terminated UTF-16 and outlives the
        // call, as does the PAC URL `options` points at (the caller's); `info` is a live, zeroed
        // struct whose strings, on success, are `GlobalAlloc`ed for us and freed by `take_global_string` below.
        let ok = unsafe { WinHttpGetProxyForUrl(self.0, target.as_ptr(), options, &mut info) };
        if ok == 0 {
            // SAFETY: no arguments; reads this thread's last error, set by the call just made.
            return Err(unsafe { GetLastError() });
        }
        // SAFETY: each pointer is null or a `GlobalAlloc`ed, NUL-terminated UTF-16 string the
        // call handed to us, read and freed once here.
        let (proxy, _bypass) = unsafe {
            (
                take_global_string(info.lpszProxy),
                take_global_string(info.lpszProxyBypass),
            )
        };
        Ok(match info.dwAccessType {
            WINHTTP_ACCESS_TYPE_NAMED_PROXY => proxy.map(|list| proxy_list_entries(&list)).unwrap_or_default(),
            WINHTTP_ACCESS_TYPE_NO_PROXY => vec![Entry::Direct],
            _ => vec![Entry::Direct],
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the session handle `open` got, closed exactly once, here.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

/// `text` as NUL-terminated UTF-16.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads and frees a `GlobalAlloc`ed UTF-16 string WinHTTP handed over.
///
/// # Safety
///
/// `ptr` is null, or a NUL-terminated UTF-16 string from `GlobalAlloc` that nothing else frees.
pub(crate) unsafe fn take_global_string(ptr: PWSTR) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller promises a NUL-terminated string, so the scan stops inside it.
    let len = (0..).take_while(|&i| unsafe { *ptr.add(i) } != 0).count();
    // SAFETY: `len` units were just read as in bounds.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) });
    // SAFETY: the caller promises `ptr` came from `GlobalAlloc` and is freed only here.
    unsafe { GlobalFree(ptr.cast()) };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WPAD for real, through the shared evaluator: however long discovery takes on this
    /// network (no WPAD server is the usual case), the asking thread waits at most its budget.
    #[test]
    fn auto_detect_answers_within_the_askers_budget() {
        use crate::entries::Pac;
        let evaluator = crate::pac::OsPac::new(crate::pac::TIMEOUT);
        let started = std::time::Instant::now();
        let answer = evaluator.evaluate(
            &PacSource::AutoDetect,
            &crate::pac::test_server::url("https://example.com"),
        );
        let took = started.elapsed();
        assert!(
            took < crate::pac::TIMEOUT + Duration::from_secs(2),
            "took {took:?}, answered {answer:?}"
        );
    }

    /// Windows' settings never name an inline script, so it has no runner.
    #[test]
    fn an_inline_script_has_no_runner() {
        assert_eq!(
            execute(
                &PacSource::Script("function FindProxyForURL() {}".into()),
                "https://example.com",
                Duration::from_secs(1)
            ),
            None
        );
    }
}
