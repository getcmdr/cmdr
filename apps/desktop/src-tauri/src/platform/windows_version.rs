//! How Windows describes itself in reports, as pure text: the Windows arm reads the parts, this
//! puts them together. Compiled on Windows, and for tests everywhere, so the naming rules run on
//! every developer's machine.

/// What the Windows arm read: the kernel's own version numbers (`RtlGetVersion`) plus the
/// registry's names for this install.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WindowsVersion {
    pub(crate) major: u32,
    pub(crate) minor: u32,
    pub(crate) build: u32,
    /// The update build revision, `UBR`: the `.6456` in `19045.6456`, which moves with each
    /// cumulative update.
    pub(crate) ubr: Option<u32>,
    /// `ProductName`, for example `Windows 10 Pro`. ❗ Still says "Windows 10" on Windows 11.
    pub(crate) product_name: Option<String>,
    /// `DisplayVersion` (`22H2`), or the older `ReleaseId` (`2009`) where it's missing.
    pub(crate) display_version: Option<String>,
}

/// The first build of Windows 11. Both report major version 10; only the build tells them apart.
const WINDOWS_11_FIRST_BUILD: u32 = 22_000;

/// For example `Windows 11 Pro 23H2 (build 22631.4460)`, `Windows 10 Pro 22H2 (build 19045.6456)`,
/// or `Windows Server 2022 Datacenter 21H2 (build 20348.2700)`. Never empty.
pub(crate) fn describe(version: &WindowsVersion) -> String {
    let product = version.product_name.as_deref().map(str::trim).filter(|p| !p.is_empty());
    let mut parts: Vec<String> = Vec::new();
    match product {
        // A server names itself fully and correctly; Windows 11's client SKUs don't.
        Some(server) if server.starts_with("Windows Server") => parts.push(server.to_string()),
        _ => {
            parts.push(family(version));
            if let Some(edition) = product.and_then(edition_of) {
                parts.push(edition.to_string());
            }
        }
    }
    if let Some(display) = version
        .display_version
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        parts.push(display.to_string());
    }
    let build = match version.ubr {
        Some(ubr) => format!("{}.{ubr}", version.build),
        None => version.build.to_string(),
    };
    parts.push(format!("(build {build})"));
    parts.join(" ")
}

/// `Windows 11`, `Windows 10`, or the bare numbers for anything else.
fn family(version: &WindowsVersion) -> String {
    match (version.major, version.minor) {
        (10, 0) if version.build >= WINDOWS_11_FIRST_BUILD => "Windows 11".to_string(),
        (10, 0) => "Windows 10".to_string(),
        (major, minor) => format!("Windows {major}.{minor}"),
    }
}

/// The edition out of a client `ProductName`: `Pro` out of `Windows 10 Pro`. `None` when the name
/// doesn't start with a family we know, so a surprise never prints twice.
fn edition_of(product: &str) -> Option<&str> {
    ["Windows 10 ", "Windows 11 "]
        .iter()
        .find_map(|prefix| product.strip_prefix(prefix))
        .map(str::trim)
        .filter(|edition| !edition.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(build: u32, product: Option<&str>, display: Option<&str>, ubr: Option<u32>) -> WindowsVersion {
        WindowsVersion {
            major: 10,
            minor: 0,
            build,
            ubr,
            product_name: product.map(str::to_string),
            display_version: display.map(str::to_string),
        }
    }

    #[test]
    fn windows_10_reads_as_itself() {
        assert_eq!(
            describe(&version(19045, Some("Windows 10 Pro"), Some("22H2"), Some(6456))),
            "Windows 10 Pro 22H2 (build 19045.6456)"
        );
    }

    /// Windows 11 keeps `ProductName` at "Windows 10 Pro"; the build is the truth.
    #[test]
    fn windows_11_is_told_apart_by_its_build() {
        assert_eq!(
            describe(&version(22631, Some("Windows 10 Pro"), Some("23H2"), Some(4460))),
            "Windows 11 Pro 23H2 (build 22631.4460)"
        );
        assert_eq!(
            describe(&version(26100, Some("Windows 10 Home"), Some("24H2"), None)),
            "Windows 11 Home 24H2 (build 26100)"
        );
    }

    #[test]
    fn a_server_keeps_its_own_name() {
        assert_eq!(
            describe(&version(
                20348,
                Some("Windows Server 2022 Datacenter"),
                Some("21H2"),
                Some(2700)
            )),
            "Windows Server 2022 Datacenter 21H2 (build 20348.2700)"
        );
    }

    #[test]
    fn missing_registry_parts_still_name_the_family_and_build() {
        assert_eq!(describe(&version(19045, None, None, None)), "Windows 10 (build 19045)");
        assert_eq!(
            describe(&version(19045, Some("  "), Some(""), None)),
            "Windows 10 (build 19045)"
        );
        // A product name we don't recognize adds no edition rather than a garbled one.
        assert_eq!(
            describe(&version(19045, Some("Something Else"), None, None)),
            "Windows 10 (build 19045)"
        );
    }

    #[test]
    fn an_older_kernel_prints_its_numbers() {
        let old = WindowsVersion {
            major: 6,
            minor: 3,
            build: 9600,
            ..WindowsVersion::default()
        };
        assert_eq!(describe(&old), "Windows 6.3 (build 9600)");
    }
}
