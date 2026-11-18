//! macOS: the update syncs into the running `.app` bundle, which the next launch runs.

pub(crate) mod bundle_location;
// Crate-visible for `running_bundle`, which `dock/`, `cli/`, `analytics/`, and
// `install_location` need to decide what this copy is. Nothing else outside `updater` reaches in.
pub(crate) mod installer;
// `lib.rs::setup` calls it on the first launch after an update (#1019).
pub(crate) mod local_network_refresh;

use std::path::{Path, PathBuf};

use cmdr_fs::log_private::LogPrivate;

use crate::updater::install_kind::{InstallKind, NotUpdatable};
use crate::updater::{BundleWriteBlocker, HandoffPlan, UpdateInstallError, VettedBundle};

pub(crate) fn current_install() -> Result<InstallKind, NotUpdatable> {
    if installer::is_running_from_app_bundle() {
        Ok(InstallKind::MacAppBundle)
    } else {
        Err(NotUpdatable::NotAnAppBundle)
    }
}

/// Classifies the running bundle (`bundle_location::classify`); `None` outside a bundle, where the
/// check gate (`skip_reason`) has already stopped the process from getting here.
pub(crate) fn write_blocker() -> Option<BundleWriteBlocker> {
    let bundle = installer::running_bundle().ok()?;
    let blocker = bundle_location::classify(&bundle);
    if let Some(reason) = blocker {
        log::warn!("Can't install updates into {}: {reason}", LogPrivate::dir_path(&bundle));
    }
    blocker
}

/// Walks up from the executable's own path; no filesystem access.
pub(crate) fn bundle_path() -> Option<PathBuf> {
    installer::running_bundle().ok()
}

/// Extracts the tarball and syncs it into the running bundle (`installer::install`).
pub(crate) fn install_bundle(
    tarball: &Path,
    may_install: &dyn Fn(&semver::Version) -> bool,
) -> Result<VettedBundle, UpdateInstallError> {
    installer::install(tarball, may_install)
}

/// Never staged on macOS: the install already wrote the bundle, so there's nothing to hand off.
pub(crate) fn hand_off<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _plan: &HandoffPlan,
    _bytes: Vec<u8>,
) -> Result<(), String> {
    Err("macOS installs into its bundle; there's no installer to hand off".to_string())
}
