//! Windows: a per-user NSIS install updates itself (the plugin runs the staged installer as Cmdr
//! exits). An MSI never checks: Cmdr ships no MSI updater artifact. Nor does the Store's MSIX: it
//! carries the exe from before the bundler stamped it, which reads as `None` here, and the Store
//! updates it (`apps/desktop/packaging/msix/DETAILS.md` § Decision/Why: the unstamped exe).

use std::path::{Path, PathBuf};

use tauri::utils::config::BundleType;

use crate::updater::install_kind::{InstallKind, NotUpdatable};
use crate::updater::{BundleWriteBlocker, UpdateInstallError, VettedBundle};

pub(crate) use super::plugin::hand_off;

pub(crate) fn current_install() -> Result<InstallKind, NotUpdatable> {
    kind_from_bundle(tauri::utils::platform::bundle_type())
}

/// The bundler's stamp. Pure, so every arm is tested.
fn kind_from_bundle(bundle: Option<BundleType>) -> Result<InstallKind, NotUpdatable> {
    match bundle {
        Some(BundleType::Nsis) => Ok(InstallKind::WindowsNsis),
        None => Err(NotUpdatable::NotAnInstalledBuild),
        Some(_) => Err(NotUpdatable::UnsupportedBundle),
    }
}

/// No pre-flight check: a per-user NSIS install writes `%LOCALAPPDATA%`, which this user owns.
pub(crate) fn write_blocker() -> Option<BundleWriteBlocker> {
    None
}

pub(crate) fn bundle_path() -> Option<PathBuf> {
    None
}

/// Windows has no bundle to sync into; `install_update` stages the installer instead.
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
    fn only_an_nsis_install_updates_itself() {
        assert_eq!(kind_from_bundle(Some(BundleType::Nsis)), Ok(InstallKind::WindowsNsis));
        assert_eq!(
            kind_from_bundle(Some(BundleType::Msi)),
            Err(NotUpdatable::UnsupportedBundle)
        );
        assert_eq!(kind_from_bundle(None), Err(NotUpdatable::NotAnInstalledBuild));
    }
}
