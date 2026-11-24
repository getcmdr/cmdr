//! Linux: no file manager treats a suffixed folder as one item, so `X.app` is a folder. Icons
//! come from the XDG theme by type, so no file reads its own.

pub(crate) const FOLDER_NAMES_MAKE_PACKAGES: bool = false;

pub(crate) const OWN_ICON_EXTENSIONS: &[&str] = &[];
