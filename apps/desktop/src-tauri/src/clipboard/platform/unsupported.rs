//! The answers of an OS with no clipboard to reach yet: Linux's file clipboard says it isn't
//! supported here, and paste-content-as-file on Linux and Windows finds nothing to paste.

use cmdr_operations::MutationError;
use tauri::{AppHandle, Runtime};

use super::super::PasteClipboardReply;
#[cfg(target_os = "linux")]
use super::super::{ClipboardFiles, ClipboardFilesError, ClipboardIntent};

/// Not supported yet.
#[cfg(target_os = "linux")]
pub(crate) fn write_file_urls(
    _app: AppHandle,
    _paths: Vec<std::path::PathBuf>,
    _intent: ClipboardIntent,
) -> Result<(), ClipboardFilesError> {
    Err(ClipboardFilesError::NotSupported)
}

/// Not supported yet.
#[cfg(target_os = "linux")]
pub(crate) fn read_file_urls(_app: AppHandle) -> Result<ClipboardFiles, ClipboardFilesError> {
    Err(ClipboardFilesError::NotSupported)
}

/// Nothing to tell: no cut mark is ever read here.
#[cfg(target_os = "linux")]
pub(crate) fn after_cut_paste(_paths: Vec<std::path::PathBuf>) {}

/// As macOS reads a file URL's path.
#[cfg(target_os = "linux")]
pub(crate) fn app_text(native: &std::path::Path) -> Option<String> {
    Some(super::posix_app_text(native))
}

/// Nothing to drop: no cut mark is ever written here.
#[cfg(target_os = "linux")]
pub(crate) fn drop_cut_mark(_paths: Vec<std::path::PathBuf>) -> Result<(), ClipboardFilesError> {
    Ok(())
}

/// Nothing to make a file of: content isn't read off the clipboard here yet, so the answer is
/// macOS's for an empty clipboard, which the frontend shows as the quiet "No files on the
/// clipboard" note. ❌ Never a refusal: a Ctrl+V on a clipboard without files is an expected
/// state, and a refusal reads as a red "couldn't create a file" toast (#1477).
pub(crate) async fn paste_as_file<R: Runtime>(
    _app: AppHandle<R>,
    _volume_id: Option<String>,
    _directory: String,
) -> Result<PasteClipboardReply, MutationError> {
    Ok(PasteClipboardReply::Done { file: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paste with no files on the clipboard ends as "nothing pasteable", never a refusal.
    #[tokio::test]
    async fn a_paste_with_no_files_finds_nothing_to_paste() {
        let app = tauri::test::mock_app();
        let reply = paste_as_file(app.handle().clone(), None, "C:/Users/test_user_1".into()).await;
        assert!(
            matches!(reply, Ok(PasteClipboardReply::Done { file: None })),
            "got {reply:?}"
        );
    }
}
