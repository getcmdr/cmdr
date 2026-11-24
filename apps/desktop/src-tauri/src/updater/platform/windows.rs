//! Windows: a per-user NSIS install updates itself (the plugin runs the staged installer as Cmdr
//! exits). An MSI never checks: Cmdr ships no MSI updater artifact. Nor does the Store's MSIX: the
//! process runs with a package identity, which answers `StorePackage` before the bundler's stamp is
//! read, and the Store updates it. The MSIX also carries the exe from before the bundler stamped
//! it, so it would read as `None` even without the identity check
//! (`apps/desktop/packaging/msix/DETAILS.md` § Decision/Why: the unstamped exe).

use std::path::{Path, PathBuf};

use tauri::utils::config::BundleType;

use crate::updater::install_kind::{InstallKind, NotUpdatable};
use crate::updater::{BundleWriteBlocker, UpdateInstallError, VettedBundle};

pub(crate) use super::plugin::hand_off;

pub(crate) fn current_install() -> Result<InstallKind, NotUpdatable> {
    kind_from(package_identity(), tauri::utils::platform::bundle_type())
}

/// Whether this process runs inside an MSIX package, as `GetCurrentPackageFullName` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageIdentity {
    /// A package identity: the Store's MSIX, or a dev package registered in Developer Mode.
    Packaged,
    /// A plain process: an NSIS install, or `cargo build` / `tauri dev`.
    Unpackaged,
}

/// The package question first, then the bundler's stamp. Pure, so every arm is tested. A packaged
/// process must never run the NSIS updater, whatever its exe carries.
fn kind_from(identity: PackageIdentity, bundle: Option<BundleType>) -> Result<InstallKind, NotUpdatable> {
    match identity {
        PackageIdentity::Packaged => Err(NotUpdatable::StorePackage),
        PackageIdentity::Unpackaged => kind_from_bundle(bundle),
    }
}

/// The bundler's stamp. Pure, so every arm is tested.
fn kind_from_bundle(bundle: Option<BundleType>) -> Result<InstallKind, NotUpdatable> {
    match bundle {
        Some(BundleType::Nsis) => Ok(InstallKind::WindowsNsis),
        None => Err(NotUpdatable::NotAnInstalledBuild),
        Some(_) => Err(NotUpdatable::UnsupportedBundle),
    }
}

fn package_identity() -> PackageIdentity {
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    let mut length: u32 = 0;
    // SAFETY: a zero length with a null buffer is the documented size query: the call writes only
    // into `length`, a live local, and reads nothing through the null name pointer.
    let status = unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) };
    identity_from_status(status)
}

/// Reads `GetCurrentPackageFullName`'s status. Anything but "no package" counts as packaged: a
/// packaged process mistaken for a plain one would stage an NSIS installer that installs a second,
/// separate Cmdr, while the other mistake only skips a self-update.
fn identity_from_status(status: u32) -> PackageIdentity {
    use windows_sys::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    match status {
        APPMODEL_ERROR_NO_PACKAGE => PackageIdentity::Unpackaged,
        ERROR_INSUFFICIENT_BUFFER | ERROR_SUCCESS => PackageIdentity::Packaged,
        other => {
            log::warn!(
                target: "updater",
                "Couldn't tell whether Cmdr runs as a package (status {other}), so leaving updates to the Store"
            );
            PackageIdentity::Packaged
        }
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

    /// A packaged process is the Store's to update, even if its exe somehow carries the NSIS stamp.
    #[test]
    fn a_store_package_never_updates_itself() {
        for bundle in [Some(BundleType::Nsis), Some(BundleType::Msi), None] {
            let label = format!("{bundle:?}");
            assert_eq!(
                kind_from(PackageIdentity::Packaged, bundle),
                Err(NotUpdatable::StorePackage),
                "{label}"
            );
        }
        assert_eq!(
            kind_from(PackageIdentity::Unpackaged, Some(BundleType::Nsis)),
            Ok(InstallKind::WindowsNsis)
        );
        assert_eq!(
            kind_from(PackageIdentity::Unpackaged, None),
            Err(NotUpdatable::NotAnInstalledBuild)
        );
    }

    #[test]
    fn only_no_package_reads_as_unpackaged() {
        use windows_sys::Win32::Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER,
        };
        assert_eq!(
            identity_from_status(APPMODEL_ERROR_NO_PACKAGE),
            PackageIdentity::Unpackaged
        );
        assert_eq!(
            identity_from_status(ERROR_INSUFFICIENT_BUFFER),
            PackageIdentity::Packaged
        );
        assert_eq!(identity_from_status(ERROR_INVALID_PARAMETER), PackageIdentity::Packaged);
    }

    /// The real call, where the tests run: CI's runner and `cargo test` are never packaged.
    #[test]
    fn a_test_process_is_unpackaged() {
        assert_eq!(package_identity(), PackageIdentity::Unpackaged);
    }
}
