//! What kind of install this process runs from, and so how an update reaches it.
//!
//! Every OS shares the check, the download, the minisign verification, the managed policy, and the
//! advisory list. What differs is the artifact and the last step:
//!
//! - **macOS `.app` bundle**: a tarball, synced into the running bundle (`installer.rs`), applied
//!   by the next launch. The bundle's own `Info.plist` binds the version.
//! - **Windows NSIS** and **Linux AppImage**: the installer (or the AppImage itself), staged
//!   verified and handed to the Tauri updater plugin when Cmdr quits to restart (`handoff.rs`).
//!   The signature's trusted comment binds the version (`--app-version`).
//!
//! Anything else (a dev binary, a `.deb` or `.rpm`, an MSI) never checks: [`NotUpdatable`].

/// An install the updater can update. Pure data, so every OS's arm is testable on any host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "each OS constructs only its own variant; every arm is tested on every OS"
)]
pub enum InstallKind {
    /// A `.app` bundle on macOS.
    MacAppBundle,
    /// A per-user NSIS install on Windows (`bundle.windows.nsis`).
    WindowsNsis,
    /// An AppImage on Linux, which the plugin replaces in place.
    LinuxAppImage,
}

/// Why this process has nothing the updater can update. Carried rather than collapsed to a bool
/// so the log names the exact condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "each OS constructs only its own variants; every arm is tested on every OS"
)]
pub enum NotUpdatable {
    /// The executable isn't inside a `.app` bundle (macOS dev builds).
    NotAnAppBundle,
    /// The binary carries no bundle marker: `cargo build` or `tauri dev` (Windows and Linux).
    NotAnInstalledBuild,
    /// A `.deb` or `.rpm` install. Its files belong to the package manager, and updating it
    /// ourselves would take an admin prompt (`pkexec`), which the updater never asks for.
    PackageManagerInstall,
    /// An install kind Cmdr doesn't ship an updater artifact for (an MSI).
    UnsupportedBundle,
}

impl std::fmt::Display for NotUpdatable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnAppBundle => f.write_str("not running from a .app bundle"),
            Self::NotAnInstalledBuild => f.write_str("not running from an installed build"),
            Self::PackageManagerInstall => f.write_str("installed by a package manager, which updates it"),
            Self::UnsupportedBundle => f.write_str("installed in a form Cmdr ships no update for"),
        }
    }
}

impl InstallKind {
    /// The `latest.json` keys this install reads, most specific first. Windows and Linux follow the
    /// Tauri plugin's own lookup (`{os}-{arch}-{installer}`, then `{os}-{arch}`), so a manifest the
    /// plugin reads, this reads too; macOS reads only `darwin-{arch}`, as it always has.
    pub fn platform_keys(self, arch: Arch) -> Vec<String> {
        let arch = arch.as_str();
        match self {
            Self::MacAppBundle => vec![format!("darwin-{arch}")],
            Self::WindowsNsis => vec![format!("windows-{arch}-nsis"), format!("windows-{arch}")],
            Self::LinuxAppImage => vec![format!("linux-{arch}-appimage"), format!("linux-{arch}")],
        }
    }

    /// The `arch` the update check sends to `api.getcmdr.com/update-check`, which the dashboard
    /// counts installs by. macOS sends the bare arch, as it always has; the others prefix their OS,
    /// so a Windows or Linux install never lands in a macOS arch's count.
    pub fn check_arch_label(self, arch: Arch) -> String {
        match self {
            Self::MacAppBundle => arch.as_str().to_string(),
            Self::WindowsNsis => format!("windows-{}", arch.as_str()),
            Self::LinuxAppImage => format!("linux-{}", arch.as_str()),
        }
    }

    /// Whether the version an update is installed as comes from the signature's trusted comment.
    /// macOS reads it from the extracted bundle's `Info.plist` instead (`installer::refuse_unless_newer`);
    /// an installer can't be opened before it runs, so the signature is the one signed place left.
    pub fn binds_version_in_signature(self) -> bool {
        match self {
            Self::MacAppBundle => false,
            Self::WindowsNsis | Self::LinuxAppImage => true,
        }
    }

    /// The file name the verified download of `version` is written to in the temp dir. Off macOS
    /// it names the version: a staged installer waits for Restart, and a newer download must not
    /// overwrite it before the newer one is staged in its place (a refused stage keeps the old).
    pub fn download_file_name(self, version: &semver::Version) -> String {
        match self {
            Self::MacAppBundle => "Cmdr.app.tar.gz".to_string(),
            Self::WindowsNsis => format!("Cmdr-{version}-setup.exe"),
            Self::LinuxAppImage => format!("Cmdr-{version}.AppImage"),
        }
    }
}

/// The CPU architectures Cmdr ships for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Aarch64,
    X86_64,
}

impl Arch {
    /// This binary's architecture.
    pub fn current() -> Self {
        if cfg!(target_arch = "aarch64") {
            Self::Aarch64
        } else {
            Self::X86_64
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64",
            Self::X86_64 => "x86_64",
        }
    }
}

/// This process's install kind, which each OS answers in `platform/`.
pub fn current() -> Result<InstallKind, NotUpdatable> {
    super::platform::current_install()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS reads exactly the keys it always has, and reports its arch to the check as before.
    #[test]
    fn macos_keeps_its_manifest_key_and_check_arch() {
        assert_eq!(
            InstallKind::MacAppBundle.platform_keys(Arch::Aarch64),
            ["darwin-aarch64"]
        );
        assert_eq!(InstallKind::MacAppBundle.platform_keys(Arch::X86_64), ["darwin-x86_64"]);
        assert_eq!(InstallKind::MacAppBundle.check_arch_label(Arch::Aarch64), "aarch64");
        assert!(!InstallKind::MacAppBundle.binds_version_in_signature());
        assert_eq!(
            InstallKind::MacAppBundle.download_file_name(&semver::Version::new(0, 53, 0)),
            "Cmdr.app.tar.gz"
        );
        assert_eq!(
            InstallKind::WindowsNsis.download_file_name(&semver::Version::new(0, 53, 0)),
            "Cmdr-0.53.0-setup.exe"
        );
    }

    #[test]
    fn windows_and_linux_follow_the_plugins_key_order() {
        assert_eq!(
            InstallKind::WindowsNsis.platform_keys(Arch::X86_64),
            ["windows-x86_64-nsis", "windows-x86_64"]
        );
        assert_eq!(
            InstallKind::LinuxAppImage.platform_keys(Arch::Aarch64),
            ["linux-aarch64-appimage", "linux-aarch64"]
        );
        assert_eq!(
            InstallKind::WindowsNsis.check_arch_label(Arch::X86_64),
            "windows-x86_64"
        );
        assert!(InstallKind::WindowsNsis.binds_version_in_signature());
        assert!(InstallKind::LinuxAppImage.binds_version_in_signature());
    }
}
