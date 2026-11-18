//! What each OS does differently in an update: which install this is, whether it can be written,
//! and the last step. Everything above the seam (the check, the policy, the signature, the
//! advisory list, staging) is shared. A platform seam (`docs/guides/platform-seams.md`).

use std::path::{Path, PathBuf};

use super::install_kind::{InstallKind, NotUpdatable};
use super::{BundleWriteBlocker, HandoffPlan, UpdateInstallError, VettedBundle};

#[cfg_attr(target_os = "macos", path = "macos/mod.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

// Windows and Linux both hand the staged installer to the Tauri updater plugin.
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod plugin;

/// The managed policy's answer for a version, as `install_bundle` asks it.
type MayInstall<'a> = &'a dyn Fn(&semver::Version) -> bool;
type InstallBundleFn = fn(&Path, MayInstall<'_>) -> Result<VettedBundle, UpdateInstallError>;
type HandOffFn = fn(&tauri::AppHandle<tauri::Wry>, &HandoffPlan, Vec<u8>) -> Result<(), String>;

const _: fn() -> Result<InstallKind, NotUpdatable> = current_install;
const _: fn() -> Option<BundleWriteBlocker> = write_blocker;
const _: fn() -> Option<PathBuf> = bundle_path;
const _: InstallBundleFn = install_bundle;
const _: HandOffFn = hand_off::<tauri::Wry>;

/// This process's install, or why nothing here can be updated (a dev build, a `.deb`).
pub(crate) use imp::current_install;

/// Why this install can't write its own update, decided before the download so a doomed one never
/// starts. Blocking (a `statfs` and a folder walk on macOS); the command bounds it. Leans "no
/// blocker": a false block stops updates that would have worked.
pub(crate) use imp::write_blocker;

/// The running bundle's path, for the notice that asks someone to replace it; `None` where there's
/// no bundle (off macOS).
pub(crate) use imp::bundle_path;

/// Installs a verified macOS tarball into the running bundle; `may_install` is the managed
/// policy's answer for the version the extracted bundle names. Refused off macOS, where
/// `install_update` stages instead and never calls it.
pub(crate) use imp::install_bundle;

/// Runs the staged installer (`../handoff.rs`): Windows exits from inside it on success; Linux
/// returns once the AppImage is replaced. Refused on macOS, where nothing is ever staged.
pub(crate) use imp::hand_off;

/// macOS's own pieces, which only same-OS code asks for (`running_bundle` for `dock/`, `cli/`,
/// `analytics/`, and `install_location`; the post-update Local Network refresh for `lib.rs`).
#[cfg(target_os = "macos")]
pub(crate) use imp::{installer, local_network_refresh};
