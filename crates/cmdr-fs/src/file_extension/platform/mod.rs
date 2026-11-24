//! How this OS reads names, per OS: whether a folder's name makes it a package (`Cmdr.app`
//! shown as one opaque item; only macOS), and which files carry an icon of their own (only
//! Windows).

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: bool = FOLDER_NAMES_MAKE_PACKAGES;
const _: &[&str] = OWN_ICON_EXTENSIONS;

/// Whether a folder named with a [`super::PACKAGE_EXTENSIONS`] suffix is a package here.
pub(super) use imp::FOLDER_NAMES_MAKE_PACKAGES;

/// File extensions (lowercase, no dot) whose files this OS draws with an icon of their own
/// rather than their type's, so each such file is its own icon key (`icons::own_icons`).
pub(super) use imp::OWN_ICON_EXTENSIONS;
