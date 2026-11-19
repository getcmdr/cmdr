//! Windows: the current user's Internet settings (`WinHttpGetIEProxyConfigForCurrentUser`, the
//! same settings Edge, Chrome, and WinINet read), walked the way WinINet walks them
//! (`ie_settings.rs`): WPAD, then the setup script, both run by WinHTTP (`crate::pac`), then the
//! manual proxy and its bypass list.
//!
//! WPAD ("Automatically detect settings", on by default) is discovered once per network in the
//! background (`wpad.rs`): a request waits for that discovery at most [`pac::TIMEOUT`], without
//! holding any lock another host's request needs, then reads the found PAC through the shared
//! cache like a setup script. The network is `windows_network.rs`'s number, which Windows' change
//! notification moves.
//!
//! The machine-wide WinHTTP setting (`netsh winhttp set proxy`) is a different, service-facing
//! setting that browsers don't read, so Cmdr doesn't either.

use std::sync::Arc;

use reqwest::Url;
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WinHttpGetIEProxyConfigForCurrentUser,
};

use super::ie_settings::{IeSettings, entries_for};
use super::wpad::{RETRY_AFTER, Wpad};
use crate::Route;
use crate::entries::{Entry, Pac, PacSource, first_route};
use crate::pac;
use crate::pac::platform::take_global_string;

#[path = "windows_network.rs"]
mod network;

pub(crate) use network::network_generation;

/// This process's WPAD discovery, one per network.
static WPAD: Wpad = Wpad::new(RETRY_AFTER);

/// The current settings' verdict for `url`, a PAC file's or WPAD's included.
pub(crate) fn route(url: &Url) -> Route {
    route_with(url, pac::shared())
}

/// [`route`], with a PAC entry run by `pac`.
pub(crate) fn route_with(url: &Url, pac: &dyn Pac) -> Route {
    let Some(settings) = current_settings() else {
        return Route::Direct;
    };
    let entries: Vec<Entry> = entries_for(&settings, url)
        .into_iter()
        .filter_map(|entry| match entry {
            // WPAD stands for the PAC it found on this network, or for nothing.
            Entry::AutoDetect => wpad_pac_url(url).map(Entry::AutoConfigUrl),
            other => Some(other),
        })
        .collect();
    first_route(&entries, url, pac)
}

/// The PAC URL WPAD found on this network, waiting at most [`pac::TIMEOUT`] for a discovery in
/// flight (and starting one when this network has none yet).
fn wpad_pac_url(url: &Url) -> Option<String> {
    let network = network_generation();
    let target = url.as_str().to_string();
    WPAD.pac_url(network, pac::TIMEOUT, move || discover_in_background(network, target))
}

/// Runs [`discover`] on a thread of its own, its answer landing in [`WPAD`].
fn discover_in_background(network: u64, target: String) {
    let spawned = std::thread::Builder::new()
        .name(String::from("cmdr-http-wpad"))
        .spawn(move || WPAD.finish(network, discover(&target)));
    if spawned.is_err() {
        log::debug!(target: "cmdr_http", "couldn't start WPAD discovery; skipping WPAD on this network");
        WPAD.finish(network, None);
    }
}

/// Finds WPAD's PAC (DHCP, then DNS), then runs it once for `target` with the warm-up's budget:
/// fetching the script is what a slow or stalling server makes slow, and once it's fetched,
/// WinHTTP's service keeps it, so each host's own evaluation after this is quick. A PAC that
/// can't be fetched or run counts as no WPAD here.
fn discover(target: &str) -> Option<String> {
    let pac_url = pac::platform::detect_pac_url()?;
    pac::platform::execute(&PacSource::Url(pac_url.clone()), target, pac::WARM_UP_TIMEOUT).map(|_| pac_url)
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
