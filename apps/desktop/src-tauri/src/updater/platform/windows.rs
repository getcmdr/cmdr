//! Windows: a per-user NSIS install updates itself (the plugin runs the staged installer as Cmdr
//! exits). An MSI never checks: Cmdr ships no MSI updater artifact. Nor does an MSIX: a package's
//! files are read-only, so it never updates itself. The package's origin decides what the UI says:
//! a Store-signed one is `StorePackage` (the Store updates it), any other (a dev package in
//! Developer Mode, a sideloaded or line-of-business one) is `SideloadedPackage`. Both are asked
//! before the bundler's stamp. The MSIX also carries the exe from before the bundler stamped it, so
//! it would read as `None` even without the package check (`apps/desktop/packaging/msix/DETAILS.md`
//! § Decision/Why: the unstamped exe).

use std::path::{Path, PathBuf};

use tauri::utils::config::BundleType;
use windows_sys::Win32::Storage::Packaging::Appx::{PackageOrigin, PackageOrigin_Store};

use crate::updater::install_kind::{InstallKind, NotUpdatable};
use crate::updater::{BundleWriteBlocker, UpdateInstallError, VettedBundle};

pub(crate) use super::plugin::hand_off;

pub(crate) fn current_install() -> Result<InstallKind, NotUpdatable> {
    kind_from(packaging(), tauri::utils::platform::bundle_type())
}

/// How this process was installed, as far as packaging goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Packaging {
    /// A plain process: an NSIS install, or `cargo build` / `tauri dev`.
    Unpackaged,
    /// An MSIX the Microsoft Store signed: the Store installed it and updates it.
    Store,
    /// Any other MSIX: a dev package registered in Developer Mode, or one sideloaded or deployed
    /// by an organization. Nothing updates it but a newer package.
    Sideloaded,
}

/// The package question first, then the bundler's stamp. Pure, so every arm is tested. A packaged
/// process must never run the NSIS updater, whatever its exe carries.
fn kind_from(packaging: Packaging, bundle: Option<BundleType>) -> Result<InstallKind, NotUpdatable> {
    match packaging {
        Packaging::Store => Err(NotUpdatable::StorePackage),
        Packaging::Sideloaded => Err(NotUpdatable::SideloadedPackage),
        Packaging::Unpackaged => kind_from_bundle(bundle),
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

/// Asks `GetCurrentPackageFullName` whether this process has a package identity, then
/// `GetStagedPackageOrigin` where that package came from (Win32, so no WinRT activation; both in
/// `appmodel.h` since Windows 8.1, Microsoft Learn, read 2026-11-25).
fn packaging() -> Packaging {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::Storage::Packaging::Appx::{
        GetCurrentPackageFullName, GetStagedPackageOrigin, PackageOrigin_Unknown,
    };
    let mut length: u32 = 0;
    // SAFETY: a zero length with a null buffer is the documented size query: the call writes only
    // into `length`, a live local, and reads nothing through the null name pointer.
    let status = unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) };
    if presence_from_status(status) == Presence::Unpackaged {
        return Packaging::Unpackaged;
    }
    // `length` counts the terminating NUL; a status other than "buffer too small" left it at 0,
    // and a one-element buffer then fails the query below, which reads as sideloaded.
    let mut name = vec![0u16; length.max(1) as usize];
    // SAFETY: `name` holds `length` u16s (at least one), the size the call was told, and lives
    // past the call; the call writes at most that many, NUL included.
    let status = unsafe { GetCurrentPackageFullName(&mut length, name.as_mut_ptr()) };
    if status != ERROR_SUCCESS {
        return packaging_from_origin(Err(status));
    }
    let mut origin: PackageOrigin = PackageOrigin_Unknown;
    // SAFETY: `name` is the NUL-terminated full name the call above wrote, alive for this call,
    // and `origin` is a live local the call writes one `PackageOrigin` into.
    let status = unsafe { GetStagedPackageOrigin(name.as_ptr(), &mut origin) };
    packaging_from_origin(if status == ERROR_SUCCESS {
        Ok(origin)
    } else {
        Err(status)
    })
}

/// Whether `GetCurrentPackageFullName` found a package identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    Unpackaged,
    Packaged,
}

/// Reads `GetCurrentPackageFullName`'s size-query status. Anything but "no package" counts as
/// packaged: a packaged process mistaken for a plain one would stage an NSIS installer that
/// installs a second, separate Cmdr, while the other mistake only skips a self-update.
fn presence_from_status(status: u32) -> Presence {
    use windows_sys::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    match status {
        APPMODEL_ERROR_NO_PACKAGE => Presence::Unpackaged,
        ERROR_INSUFFICIENT_BUFFER | ERROR_SUCCESS => Presence::Packaged,
        other => {
            log::warn!(target: "updater", "Couldn't tell whether Cmdr runs as a package (status {other}), so it won't update itself");
            Presence::Packaged
        }
    }
}

/// Only a Store-signed package is the Store's. A package whose origin can't be read is treated as
/// sideloaded: it still never updates itself, and the UI makes no claim about the Store.
fn packaging_from_origin(origin: Result<PackageOrigin, u32>) -> Packaging {
    match origin {
        Ok(origin) if origin == PackageOrigin_Store => Packaging::Store,
        Ok(_) => Packaging::Sideloaded,
        Err(status) => {
            log::warn!(target: "updater", "Couldn't read where Cmdr's package came from (status {status}), so treating it as sideloaded");
            Packaging::Sideloaded
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

    /// A packaged process never runs the NSIS updater, even if its exe somehow carries the stamp:
    /// a Store-signed package is the Store's, any other package is sideloaded.
    #[test]
    fn a_package_never_updates_itself() {
        for bundle in [Some(BundleType::Nsis), Some(BundleType::Msi), None] {
            let label = format!("{bundle:?}");
            assert_eq!(
                kind_from(Packaging::Store, bundle.clone()),
                Err(NotUpdatable::StorePackage),
                "{label}"
            );
            assert_eq!(
                kind_from(Packaging::Sideloaded, bundle),
                Err(NotUpdatable::SideloadedPackage),
                "{label}"
            );
        }
        assert_eq!(
            kind_from(Packaging::Unpackaged, Some(BundleType::Nsis)),
            Ok(InstallKind::WindowsNsis)
        );
        assert_eq!(
            kind_from(Packaging::Unpackaged, None),
            Err(NotUpdatable::NotAnInstalledBuild)
        );
    }

    #[test]
    fn only_no_package_reads_as_unpackaged() {
        use windows_sys::Win32::Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER,
        };
        assert_eq!(presence_from_status(APPMODEL_ERROR_NO_PACKAGE), Presence::Unpackaged);
        assert_eq!(presence_from_status(ERROR_INSUFFICIENT_BUFFER), Presence::Packaged);
        assert_eq!(presence_from_status(ERROR_INVALID_PARAMETER), Presence::Packaged);
    }

    /// Only the Store's signature makes a Store package; a dev package (Developer Mode), a
    /// sideloaded or line-of-business one, and an origin that can't be read are all sideloaded.
    #[test]
    fn only_a_store_signed_package_is_the_stores() {
        use windows_sys::Win32::Foundation::ERROR_NOT_FOUND;
        use windows_sys::Win32::Storage::Packaging::Appx::{
            PackageOrigin_DeveloperSigned, PackageOrigin_DeveloperUnsigned, PackageOrigin_Inbox,
            PackageOrigin_LineOfBusiness, PackageOrigin_Unknown, PackageOrigin_Unsigned,
        };
        assert_eq!(packaging_from_origin(Ok(PackageOrigin_Store)), Packaging::Store);
        for origin in [
            PackageOrigin_DeveloperUnsigned,
            PackageOrigin_DeveloperSigned,
            PackageOrigin_LineOfBusiness,
            PackageOrigin_Unsigned,
            PackageOrigin_Inbox,
            PackageOrigin_Unknown,
        ] {
            assert_eq!(
                packaging_from_origin(Ok(origin)),
                Packaging::Sideloaded,
                "origin {origin}"
            );
        }
        assert_eq!(packaging_from_origin(Err(ERROR_NOT_FOUND)), Packaging::Sideloaded);
    }

    /// The real calls, where the tests run: CI's runner and `cargo test` are never packaged.
    #[test]
    fn a_test_process_is_unpackaged() {
        assert_eq!(packaging(), Packaging::Unpackaged);
    }
}
