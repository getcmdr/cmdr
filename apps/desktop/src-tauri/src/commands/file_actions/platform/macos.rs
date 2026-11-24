//! macOS: Finder reveals, Finder's Get Info window, and the text editor LaunchServices names.

use cmdr_fs::deadline::blocking_typed_result_with_timeout;
use tokio::time::Duration;

use crate::file_system::get_info::GetInfoError;
use crate::file_system::text_editor::{EditorOpenReport, OpenInEditorError};

/// Get Info is one no-prompt permission read and one `spawn` of `osascript`. The
/// window, or the first-time prompt, appears after the command has answered.
const GET_INFO_TIMEOUT: Duration = Duration::from_secs(5);

/// Opening a file is one `spawn` of `open`; the wait is for the installed-app
/// lookup in front of it and the name read behind it.
const OPEN_IN_EDITOR_TIMEOUT: Duration = Duration::from_secs(5);

/// A volume's Trash is a folder a pane lists (`get_trash_dir`), so there's no view to open.
pub(crate) fn open_trash_view(
    _path: &cmdr_fs::app_path::AppPath,
) -> Result<super::TrashView, cmdr_operations::MutationError> {
    Ok(super::TrashView::NoView)
}

/// Reveals `path` in Finder. The E2E build records it instead: Finder would take the front
/// position and write a `.DS_Store` into the fixture tree.
/// Finder shows it from a thread of its own, so this answers once that thread has started.
pub(crate) async fn reveal_in_file_manager(path: String) -> Result<(), super::RevealError> {
    // A path with no native form (a scheme path, a `..`) names nothing Finder can show.
    let native = crate::commands::wire_path::native(&path).map_err(|_| super::RevealError::NoPath)?;
    let path = native.to_string_lossy().into_owned();
    #[cfg(feature = "playwright-e2e")]
    {
        crate::open_mock::record(path);
        Ok(())
    }
    #[cfg(not(feature = "playwright-e2e"))]
    {
        crate::reveal::finder_handoff::reveal_in_finder(&path).map_err(|why| {
            log::warn!(target: "reveal", "Finder wasn't asked to show the item: {why}");
            super::RevealError::NotShown
        })
    }
}

/// Finder's Get Info window, under a timeout.
pub(crate) async fn get_info(path: String) -> Result<(), GetInfoError> {
    blocking_typed_result_with_timeout(
        GET_INFO_TIMEOUT,
        || GetInfoError::TimedOut,
        |detail| {
            crate::log_error!(target: "file_actions", "get_info panicked: {detail}");
            GetInfoError::TimedOut
        },
        // A path with no native form has no Finder window to ask for: nothing launches.
        move || match crate::commands::wire_path::native(&path) {
            Ok(native) => crate::file_system::get_info::open_get_info(&native),
            Err(_) => Err(GetInfoError::LaunchRefused { errno: None }),
        },
    )
    .await
}

/// The chosen editor, through `file_system::text_editor` (which records instead of launching in
/// the E2E build).
pub(crate) async fn open_in_editor(
    path: String,
    app_choice: String,
    ask_about_other_editors: bool,
) -> Result<EditorOpenReport, OpenInEditorError> {
    blocking_typed_result_with_timeout(
        OPEN_IN_EDITOR_TIMEOUT,
        || OpenInEditorError::TimedOut,
        |detail| {
            crate::log_error!(target: "file_actions", "open_in_editor panicked: {detail}");
            OpenInEditorError::TimedOut
        },
        // A path with no native form has no file an editor opens: nothing launches.
        move || match crate::commands::wire_path::native(&path) {
            Ok(native) => {
                crate::file_system::text_editor::open_in_editor(&native, &app_choice, ask_about_other_editors)
            }
            Err(_) => Err(OpenInEditorError::LaunchRefused { errno: None }),
        },
    )
    .await
}
