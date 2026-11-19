//! Windows: the current user's Internet settings (`WinHttpGetIEProxyConfigForCurrentUser`, the
//! same settings Edge, Chrome, and WinINet read), walked the way WinINet walks them
//! (`ie_settings.rs`): WPAD, then the setup script, both run by WinHTTP (`crate::pac`), then the
//! manual proxy and its bypass list.
//!
//! The machine-wide WinHTTP setting (`netsh winhttp set proxy`) is a different, service-facing
//! setting that browsers don't read, so Cmdr doesn't either.

use std::sync::Arc;

use reqwest::Url;
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WinHttpGetIEProxyConfigForCurrentUser,
};

use super::ie_settings::{IeSettings, entries_for};
use crate::Route;
use crate::entries::{Pac, first_route};
use crate::pac;
use crate::pac::platform::take_global_string;

/// The current settings' verdict for `url`, a PAC file's or WPAD's included.
pub(crate) fn route(url: &Url) -> Route {
    route_with(url, pac::shared())
}

/// [`route`], with a PAC entry run by `pac`.
pub(crate) fn route_with(url: &Url, pac: &dyn Pac) -> Route {
    let Some(settings) = current_settings() else {
        return Route::Direct;
    };
    first_route(&entries_for(&settings, url), url, pac)
}

/// Runs the PAC (or WPAD) evaluation for `url` once, on a background thread.
pub(crate) fn warm_up(url: Url, env: Arc<crate::env::EnvProxies>) {
    pac::warm_up(url, env, route_with);
}

/// This user's Internet settings, or `None` when Windows won't say (logged at debug).
pub(crate) fn current_settings() -> Option<IeSettings> {
    let mut config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    // SAFETY: `config` is a live, writable, zeroed struct of the type the call fills; on success
    // its three strings are `GlobalAlloc`ed for the caller, which `take_global_string` frees
    // below, each once.
    let ok = unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) };
    if ok == 0 {
        log::debug!(
            target: "cmdr_http",
            "couldn't read the Internet settings: {}",
            std::io::Error::last_os_error()
        );
        return None;
    }
    // SAFETY: each pointer is null or a `GlobalAlloc`ed, NUL-terminated UTF-16 string the call
    // handed to us, read and freed once here and never touched after.
    let (auto_config_url, proxy, bypass) = unsafe {
        (
            take_global_string(config.lpszAutoConfigUrl),
            take_global_string(config.lpszProxy),
            take_global_string(config.lpszProxyBypass),
        )
    };
    Some(IeSettings {
        auto_detect: config.fAutoDetect != 0,
        auto_config_url,
        proxy,
        bypass,
    })
}

#[cfg(test)]
#[path = "windows_test.rs"]
mod tests;
