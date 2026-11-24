//! The answers of an OS with no clipboard to reach yet: each says it isn't supported here.
//! Linux's file clipboard, and paste-content-as-file on Linux and Windows.

use cmdr_operations::MutationError;
use tauri::AppHandle;

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

/// Not supported yet.
pub(crate) async fn paste_as_file(
    _app: AppHandle,
    _volume_id: Option<String>,
    _directory: String,
) -> Result<PasteClipboardReply, MutationError> {
    Err(MutationError::Volume {
        error: cmdr_fs::volume::VolumeError::NotSupported,
    })
}
