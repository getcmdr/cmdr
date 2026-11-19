//! Picks this OS's proxy reader.

use std::sync::Arc;

use reqwest::Url;

use crate::Route;
use crate::env::EnvProxies;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

// Windows' Internet settings as entry lists: pure, so every OS's tests run its rules. The
// Windows arm reads through it.
pub(crate) mod ie_settings;

// WPAD's discovery, once per network in the background, with a bounded wait for its askers:
// pure, so every OS's tests run it. The Windows arm drives it.
pub(crate) mod wpad;

const _: fn(&Url) -> Route = route;
const _: fn(Url, Arc<EnvProxies>) = warm_up;
const _: fn() -> u64 = network_generation;

/// The current system settings' verdict for `url`, a PAC file's included.
pub(super) use imp::route;

/// Runs the OS's slow first evaluation for `url` on a background thread, so a request doesn't
/// wait on it; nothing where the OS has no such step.
pub(super) use imp::warm_up;

/// Which network the machine is on, as a number that moves on every change, so what was worked
/// out for one network (a PAC's answers, WPAD's discovery) is never reused on the next. Windows
/// subscribes to the OS's change notification; elsewhere it never moves (macOS's settings carry
/// the network's own answer, Linux reads none).
pub(crate) use imp::network_generation;
