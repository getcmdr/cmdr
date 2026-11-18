//! Reads this Windows' version: the numbers from the kernel, the names from the registry.
//!
//! `RtlGetVersion` (documented in the WDK, exported by `ntdll`) answers the real version.
//! ❌ Not `GetVersionEx`: it's deprecated and answers through the app-compatibility shim, which
//! says Windows 8 (6.2) to any binary whose manifest doesn't list the newer OS GUIDs.

use super::super::windows_version::WindowsVersion;
use super::registry::{HKEY_LOCAL_MACHINE, read_dword, read_string};
use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

const CURRENT_VERSION: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";

pub(super) fn read() -> WindowsVersion {
    let (major, minor, build) = kernel_version().unwrap_or_else(|| {
        log::warn!("RtlGetVersion failed; reading the build number from the registry");
        let build = read_string(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "CurrentBuildNumber")
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        (10, 0, build)
    });
    WindowsVersion {
        major,
        minor,
        build,
        ubr: read_dword(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "UBR"),
        product_name: read_string(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "ProductName"),
        display_version: read_string(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "DisplayVersion")
            .or_else(|| read_string(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "ReleaseId")),
    }
}

/// Major, minor, and build from the kernel, or `None` if the call fails (it doesn't, in practice).
fn kernel_version() -> Option<(u32, u32, u32)> {
    // SAFETY: an all-zero `OSVERSIONINFOW` is a valid value of this plain C struct (integers and a
    // `u16` array).
    let mut info: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    info.dwOSVersionInfoSize = u32::try_from(size_of::<OSVERSIONINFOW>()).ok()?;
    // SAFETY: `info` is a live, writable `OSVERSIONINFOW` whose size field names its own size,
    // which is what `RtlGetVersion` requires before it fills the rest in.
    let status = unsafe { RtlGetVersion(&raw mut info) };
    (status == 0).then_some((info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kernel and the registry agree on the build, so neither is a shimmed answer.
    #[test]
    fn the_kernel_build_matches_the_registry() {
        let (major, _, build) = kernel_version().expect("RtlGetVersion");
        assert_eq!(major, 10, "every supported Windows is NT 10");
        let registry_build: u32 = read_string(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "CurrentBuildNumber")
            .expect("CurrentBuildNumber")
            .parse()
            .expect("a number");
        assert_eq!(build, registry_build);
    }

    #[test]
    fn this_windows_describes_itself_with_its_build() {
        let described = super::super::os_version();
        // Shown with `--no-capture`, as the evidence of what this machine reports.
        let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("os_version: {described}\n"));
        let (_, _, build) = kernel_version().expect("RtlGetVersion");
        assert!(described.starts_with("Windows "), "{described}");
        assert!(described.contains(&format!("(build {build}")), "{described}");
    }
}
