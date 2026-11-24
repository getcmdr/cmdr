//! Windows: Explorer has no bundles, so `X.app` is a folder. Explorer draws some files with the
//! icon each one holds or points at, so each of those is keyed by its path.

pub(crate) const FOLDER_NAMES_MAKE_PACKAGES: bool = false;

/// Programs (`exe`, `scr`, `cpl`), shortcuts (`lnk`, ClickOnce's `appref-ms`), and icon and
/// cursor files (`ico`, `cur`, `ani`). ❌ Not `url`: an Internet shortcut's `IconFile=` can name
/// `\\host\…`, and drawing it would open an SMB connection (and offer credentials) on listing.
pub(crate) const OWN_ICON_EXTENSIONS: &[&str] = &["ani", "appref-ms", "cpl", "cur", "exe", "ico", "lnk", "scr"];
