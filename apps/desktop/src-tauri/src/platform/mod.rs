//! Platform identity: which OS this is, and which version. Shared by the diagnostics and
//! analytics pipelines (crash reports, error reports, the heartbeat). Each OS's startup
//! installers sit beside it in `startup/`.
//!
//! Also what this OS lets Cmdr offer (`PlatformCapabilities`), how its own apps behave
//! (`PlatformConventions`), and the init script that tells every webview all three
//! (`frontend_init_script`).
//!
//! A platform seam (`docs/guides/platform-seams.md`): one arm per OS, picked below, and every
//! signature the arms owe pinned, so an arm that drifts fails to compile at the pin.

mod capabilities;
mod conventions;
mod frontend;

pub(crate) use capabilities::PlatformCapabilities;
pub(crate) use conventions::{LinuxDesktop, MenuModel, PlatformConventions};
pub(crate) use frontend::init_script as frontend_init_script;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux/mod.rs")]
#[cfg_attr(target_os = "windows", path = "windows/mod.rs")]
mod imp;

pub(crate) mod startup;

// Pure text, so its tests run on every OS.
#[cfg(any(target_os = "windows", test))]
mod windows_version;

// Pure text too: the path the Shell takes for an app path.
#[cfg(any(target_os = "windows", test))]
pub(crate) mod windows_shell_path;

// Pure text too: which handler is a type's default, for "Open with" and F4's editors.
#[cfg(any(target_os = "windows", test))]
pub(crate) mod windows_assoc_rules;

const _: fn() -> String = os_version;
const _: fn(i64, i64) -> bool = macos_at_least;
const _: fn() -> PlatformCapabilities = capabilities;
const _: fn() -> PlatformConventions = conventions;
const _: fn(&tauri::Webview<tauri::Wry>) = prepare_webview;

/// A human-readable OS version string, for example `macOS 26.0` or `Ubuntu 24.04 LTS`.
///
/// Never empty: each arm falls back to a generic label rather than failing (the heartbeat
/// contract requires a value).
pub(crate) use imp::os_version;

/// This build's OS as Rust names it: `macos`, `windows`, or `linux`. The `os` field the heartbeat,
/// feedback, crash reports, and error reports send, spelled the way the release manifest and the
/// api-server spell it (`apps/api-server/src/client-os.ts`), so the dashboard splits by OS without
/// reading the free-text [`os_version`].
pub(crate) fn os_name() -> String {
    std::env::consts::OS.to_string()
}

/// True when the running macOS is at least `major.minor`; always false off macOS.
///
/// This is the runtime half of the deployment-floor story. `objc2` carries no availability
/// information, so a selector the running OS doesn't have raises `NSInvalidArgumentException`,
/// which aborts the process instead of returning an error
/// (`scripts/check/checks/desktop-rust-macos-availability.go` has the full account). The bundle's
/// floor is macOS 10.15, so anything newer than that has to ask here first. Off macOS every caller
/// is itself macOS-only today; the question stays askable everywhere so none needs its own `cfg`.
pub(crate) use imp::macos_at_least;

/// What this OS lets Cmdr offer, every field answered by the arm. Read once per webview, by the
/// init script.
pub(crate) use imp::capabilities;

/// How this platform's own apps behave, for the desktop this session runs (Linux answers per
/// desktop). Read once per webview, by the init script.
pub(crate) use imp::conventions;

/// Readies a new webview before it's used: on Windows, WebView2's browser accelerator keys go off
/// (`windows/webview.rs`); nothing elsewhere. Called for every webview from the `cmdr-platform`
/// plugin's `on_webview_ready` (`tauri_builder.rs`).
pub(crate) use imp::prepare_webview;

/// The Linux distribution, for package-manager install hints. Linux-only by nature, so only Linux
/// code reaches it.
#[cfg(target_os = "linux")]
pub(crate) use imp::distro::LinuxDistro;

/// Registry reads in the 64-bit view, for the Windows arms of other seams (managed policy).
#[cfg(target_os = "windows")]
pub(crate) use imp::registry;

/// The single-threaded COM apartment every Shell call asks for (`windows/apartment.rs`).
#[cfg(target_os = "windows")]
pub(crate) use imp::apartment;

/// Where the system folder is, and whether a program path starts something (`windows/files.rs`).
#[cfg(target_os = "windows")]
pub(crate) use imp::files;

/// The apps Windows offers for a file type, and handing files to one (`windows/assoc.rs`).
#[cfg(target_os = "windows")]
pub(crate) use imp::assoc;

/// The Shell calls that open a file in its default app and show one in File Explorer.
#[cfg(target_os = "windows")]
pub(crate) use imp::shell;

#[cfg(test)]
mod tests {
    use super::*;

    /// The api-server knows exactly these three (`apps/api-server/src/client-os.ts`); any other
    /// value it stores as unknown.
    #[test]
    fn os_name_is_one_the_api_server_knows() {
        assert!(["macos", "windows", "linux"].contains(&os_name().as_str()));
    }

    #[test]
    fn os_version_is_never_empty() {
        assert!(!os_version().is_empty());
    }
}
