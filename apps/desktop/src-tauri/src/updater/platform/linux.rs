//! Linux: an AppImage updates itself (the plugin replaces the file `APPIMAGE` names); a `.deb` or
//! `.rpm` belongs to the package manager and never checks.

use std::path::{Path, PathBuf};

use tauri::utils::config::BundleType;

use crate::updater::install_kind::{InstallKind, NotUpdatable};
use crate::updater::{BundleWriteBlocker, UpdateInstallError, VettedBundle};

pub(crate) use super::plugin::hand_off;

pub(crate) fn current_install() -> Result<InstallKind, NotUpdatable> {
    kind_from_bundle(
        tauri::utils::platform::bundle_type(),
        std::env::var_os("APPIMAGE").is_some(),
    )
}

/// The bundler's stamp, plus whether this runs as an AppImage. Pure, so every arm is tested.
fn kind_from_bundle(bundle: Option<BundleType>, appimage_env: bool) -> Result<InstallKind, NotUpdatable> {
    match bundle {
        // The plugin replaces the file `APPIMAGE` names; without it there's nothing to replace.
        Some(BundleType::AppImage) if appimage_env => Ok(InstallKind::LinuxAppImage),
        Some(BundleType::AppImage) | None => Err(NotUpdatable::NotAnInstalledBuild),
        Some(BundleType::Deb | BundleType::Rpm) => Err(NotUpdatable::PackageManagerInstall),
        Some(_) => Err(NotUpdatable::UnsupportedBundle),
    }
}

/// No pre-flight check: an AppImage the plugin can't replace fails the hand-off at exit (logged,
/// and Cmdr restarts as it was).
pub(crate) fn write_blocker() -> Option<BundleWriteBlocker> {
    None
}

pub(crate) fn bundle_path() -> Option<PathBuf> {
    None
}

/// Linux has no bundle to sync into; `install_update` stages the AppImage instead.
pub(crate) fn install_bundle(
    _tarball: &Path,
    _may_install: &dyn Fn(&semver::Version) -> bool,
) -> Result<VettedBundle, UpdateInstallError> {
    Err(UpdateInstallError::Failed {
        detail: "a .app bundle only exists on macOS".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_running_appimage_updates_itself() {
        assert_eq!(
            kind_from_bundle(Some(BundleType::AppImage), true),
            Ok(InstallKind::LinuxAppImage)
        );
        assert_eq!(
            kind_from_bundle(Some(BundleType::AppImage), false),
            Err(NotUpdatable::NotAnInstalledBuild)
        );
        for package in [BundleType::Deb, BundleType::Rpm] {
            assert_eq!(
                kind_from_bundle(Some(package), true),
                Err(NotUpdatable::PackageManagerInstall)
            );
        }
        assert_eq!(kind_from_bundle(None, true), Err(NotUpdatable::NotAnInstalledBuild));
    }
}
