//! Picks this OS's fault reader: which region a fault address lies in, and whether it's a loaded
//! image's. Both Unixes share the SIGBUS entry point (`unix.rs`); Windows recovers nothing yet.

#[cfg(unix)]
mod unix;

// Pure, so it's compiled (and tested) on every host, though only the Unix arms ask it.
#[cfg_attr(target_os = "windows", allow(dead_code, reason = "Windows recovers nothing yet"))]
pub(super) mod verdict;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn() = prepare;
#[cfg(any(test, feature = "testing"))]
const _: fn() = install_test_handler;

/// Reads the page size and, on macOS, starts tracking every loaded image's address range.
pub(super) use imp::prepare;

/// A fault handler for a test process with no crash reporter.
#[cfg(any(test, feature = "testing"))]
pub(super) use imp::install_test_handler;

/// The SIGBUS entry point, a Unix-only extra (its signature is `siginfo_t`'s).
#[cfg(unix)]
pub use unix::recover;

/// macOS: what the table and the region reader decide for an address, for the tests.
#[cfg(all(test, target_os = "macos"))]
pub(super) use imp::{is_in_image, would_recover};
