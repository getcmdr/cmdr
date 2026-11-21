//! The frontend's one door from native path text to app text: a path the OS handed the
//! WEBVIEW (a drop's `paths`, a folder picker's or Save panel's answer) is the OS's own
//! spelling, `C:\Users\…` on Windows, which is no app path (#1311). The frontend sends it
//! here and gets app text back (`C:/Users/…`), or a typed refusal.
//!
//! On macOS and Linux an OS-reported path is its own app text, so the answer is the text
//! sent (an absolute path with no `..` name; the OS never hands one over).
//!
//! The crossing is `AppPathBuf::from_native_unicode`: no escape is made, and a name that
//! would need one is refused. The text arrives as a JS string, so it's valid Unicode by
//! construction; only a literal escape-range code point (U+10E800–U+10FEFF) can trip that.

use std::path::Path;

use cmdr_fs::app_path::{AppPathBuf, NotAnAppPath};
use cmdr_fs::log_private::LogPrivate;

/// One native path's answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativePathCrossing {
    /// The app path naming the same place.
    App {
        /// The app text (`C:/Users/x/a.txt`, `//server/share/x`, `/Users/x/a.txt`).
        path: String,
    },
    /// No app path names this text: a device namespace (`\\?\Volume{…}`), a relative or
    /// drive-relative path, or a name that would need an escape. The caller leaves it out
    /// and says so; ❌ it never sends the native text on as if it were app text.
    Refused,
}

/// The app paths for `paths`, native text the OS handed the webview, one answer per path in
/// the order sent. Pure text work, no I/O: async so a 50,000-file drop parses off the main
/// thread.
#[tauri::command]
#[specta::specta]
pub async fn app_paths_from_native(paths: Vec<String>) -> Vec<NativePathCrossing> {
    paths
        .iter()
        .map(|text| answer(text, AppPathBuf::from_native_unicode(Path::new(text))))
        .collect()
}

/// The answer for `text`, given its crossing. Split from the crossing so a test hands in
/// Windows' crossing on any host (`native_paths_test.rs`).
pub(crate) fn answer(text: &str, crossed: Result<AppPathBuf, NotAnAppPath>) -> NativePathCrossing {
    match crossed {
        Ok(app) => NativePathCrossing::App {
            path: app.into_string(),
        },
        Err(why) => {
            log::warn!(
                target: "native_paths",
                "the OS handed over a path that names no app path ({why}): left out path={:?}",
                LogPrivate::path(text)
            );
            NativePathCrossing::Refused
        }
    }
}

#[cfg(test)]
#[path = "native_paths_test.rs"]
mod native_paths_test;
