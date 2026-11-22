//! NATIVE paths that reach the app as text or an `OsString` from outside it: an environment
//! variable, a launch argument, a config value, a test's own native fixture literal. Their
//! spelling is the OS's, so they name the native path as is.
//!
//! ❌ Never app text (a row's path, a pane's location, anything the frontend sent): that reads
//! through `crate::app_text` (or `commands::wire_path` at the command edge), so a name holding an
//! escape crosses once, typed. Mount points the mount table hands over have their own door,
//! `crate::volumes::os_mount_path`. One named crossing per kind keeps every native reading of
//! text greppable, so the escape gate (`docs/specs/app-path-plan.md` § "Keeping it that way")
//! can tell them from app text turned into a `std::path` value.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// `text`, which the OS or the person's environment spelled, as the native path it names.
pub(crate) fn os_path(text: &str) -> &Path {
    Path::new(text)
}

/// `value`, an `OsString` the OS handed over (an environment variable, `argv`, a process's
/// executable), as the native path it names.
pub(crate) fn os_path_buf(value: impl Into<OsString>) -> PathBuf {
    PathBuf::from(value.into())
}

/// The native path the environment variable `name` holds, `None` when it's unset or empty.
#[cfg_attr(
    not(any(debug_assertions, test, feature = "playwright-e2e")),
    allow(dead_code, reason = "the overrides it reads are dev, test, and E2E only")
)]
pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(os_path_buf)
}
