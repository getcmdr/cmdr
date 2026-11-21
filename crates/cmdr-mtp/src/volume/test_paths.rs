//! The typed paths a cell hands the volume: a row's own path, and a name under a folder.

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic};

/// A row's `FileEntry.path` (`/Documents/␀7/notes.txt`), as the pane hands it back.
pub(super) fn row(text: &str) -> AppPathBuf {
    AppPath::from_normalized(text)
        .unwrap_or_else(|why| panic!("{text:?} is a row path: {why}"))
        .to_owned()
}

/// `name` in `folder`.
pub(super) fn child(folder: &AppPath, name: &str) -> AppPathBuf {
    folder
        .join(AppName::from_app_spelling(name).expect("one plain name"))
        .expect("a name joins its folder")
}
