//! How each OS reveals a file, opens its trash where the trash is a view of the OS's own, shows
//! its Get Info window, and opens it in a text editor. A platform seam (`docs/guides/platform-seams.md`).

use std::future::Future;

use crate::file_system::get_info::GetInfoError;
use crate::file_system::text_editor::{EditorOpenReport, OpenInEditorError};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(&cmdr_fs::app_path::AppPath) -> Result<TrashView, cmdr_operations::MutationError> = open_trash_view;

/// What [`open_trash_view`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TrashView {
    /// The OS's file manager opened the trash in a window of its own.
    #[cfg_attr(
        any(target_os = "macos", target_os = "linux"),
        allow(dead_code, reason = "only Windows' Recycle Bin is a view of the OS's own")
    )]
    Opened,
    /// No such view of this trash: a pane goes to its folder instead (`get_trash_dir`), or
    /// the drive keeps no trash.
    NoView,
}

/// Why a file didn't show in the OS's file manager. The frontend words every variant
/// (`lib/file-explorer/file-manager-reveal.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RevealError {
    /// The path names nothing the file manager can show: a `..` name, a scheme path, or text
    /// that's no app path. Nothing was asked.
    NoPath,
    /// Windows: the path has no plain spelling the Shell reads as this same item (past 259
    /// characters, a name ending in a dot or space, a device name like `CON`). Nothing was asked.
    #[cfg_attr(
        any(target_os = "macos", target_os = "linux"),
        allow(dead_code, reason = "only Windows' Shell needs the plain spelling")
    )]
    NoPlainPath,
    /// The file manager was asked and said no: the item is gone or its share unreachable, or the
    /// launch couldn't start.
    #[cfg_attr(
        feature = "playwright-e2e",
        allow(
            dead_code,
            reason = "the E2E build records a reveal on macOS and Windows instead of asking the file manager"
        )
    )]
    NotShown,
}

/// "Go to trash" where the trash is a view the OS draws rather than a folder a pane lists:
/// Windows' Recycle Bin, one Shell view of every drive's bin, opens in File Explorer when
/// `path`'s drive keeps one. [`TrashView::NoView`] on macOS and Linux, with no I/O.
pub(super) use imp::open_trash_view;

// An `async fn` doesn't coerce to a fn pointer, so these pins name each one's arguments and the
// output its future resolves to.
const _: fn() = || {
    fn resolves_to<T, F: Future<Output = T>>(_: F) {}
    resolves_to::<Result<(), RevealError>, _>(reveal_in_file_manager(String::new()));
    resolves_to::<Result<(), GetInfoError>, _>(get_info(String::new()));
    resolves_to::<Result<EditorOpenReport, OpenInEditorError>, _>(open_in_editor(String::new(), String::new(), false));
};

/// Shows `path` in the OS's file manager, or says why it didn't.
pub(super) use imp::reveal_in_file_manager;

/// Opens the OS's Get Info window for `path`, or `Ok` with nothing to show where it has none.
pub(super) use imp::get_info;

/// Opens `path` in the text editor `app_choice` names, falling back to the OS's default.
pub(super) use imp::open_in_editor;
