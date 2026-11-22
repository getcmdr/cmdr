//! App-path TEXT read as the app path it spells, for any layer: a pane's row, a stored path, a
//! path the frontend sent. The command edge's door (`commands::wire_path`) reads through here,
//! and so do the subsystems below it that get rows as `String`s (the sync-status badges, a
//! favorite's folder), so there's one reading of app text, ❌ never a `Path::new` of it.
//!
//! While escapes aren't live (`ESCAPES_LIVE`), an escape-range code point in the text is
//! LITERAL (no producer made it), so it parses through `from_literal_store`, and the native
//! path out is byte for byte the text in.

use std::path::PathBuf;

use cmdr_fs::app_path::{AppPathBuf, ESCAPES_LIVE, Namer, NotAnAppPath};

/// `text` as an app path, with this host's flavor, normalized (a trailing `/` and `.` names
/// dropped).
///
/// # Errors
///
/// [`NotAnAppPath`]: relative or empty text, a `..` name, the other OS's shape.
pub(crate) fn parse(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    if ESCAPES_LIVE {
        AppPathBuf::from_wire(text)
    } else {
        AppPathBuf::from_literal_store(text, Namer::HOST)
    }
}

/// Whether a ROUTE serves the place `text` names (inside an archive, a repo's `.git` trees, a
/// snapshot), asked of its app path. Text that's no app path routes nowhere.
pub(crate) fn routes_over_its_parent(text: &str) -> bool {
    parse(text).is_ok_and(|path| crate::file_system::volume::manager::app_path_routes_over_its_parent(&path))
}

/// The native file `text` names: [`parse`], then `to_native`. `None` for text that's no app
/// path and for an app path this OS's files don't hold (a scheme path).
pub(crate) fn native(text: &str) -> Option<PathBuf> {
    parse(text).ok()?.to_native().ok()
}
