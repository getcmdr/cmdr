//! macOS: LaunchServices presents a bundle-suffixed folder as one item. A file shows its
//! type's icon (a pasted custom file icon is rare enough to leave out), so none reads its own.

pub(crate) const FOLDER_NAMES_MAKE_PACKAGES: bool = true;

pub(crate) const OWN_ICON_EXTENSIONS: &[&str] = &[];
