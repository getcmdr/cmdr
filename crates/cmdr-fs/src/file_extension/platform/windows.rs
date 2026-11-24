//! Windows: Explorer has no bundles, so `X.app` is a folder. Explorer draws some files with the
//! icon each one holds or points at, so each of those is keyed by its path.

pub(crate) const FOLDER_NAMES_MAKE_PACKAGES: bool = false;

/// Programs (`exe`, `scr`, `cpl`), shortcuts (`lnk`, `url`, ClickOnce's `appref-ms`), and icon
/// and cursor files (`ico`, `cur`, `ani`).
pub(crate) const OWN_ICON_EXTENSIONS: &[&str] = &["ani", "appref-ms", "cpl", "cur", "exe", "ico", "lnk", "scr", "url"];
