//! How each OS reaches its clipboard. A platform seam (`docs/guides/platform-seams.md`); the
//! commands above it (`commands/clipboard.rs`) and the cut state are portable.

use std::future::Future;
use std::path::PathBuf;

use cmdr_operations::MutationError;
use tauri::AppHandle;

use super::{ClipboardFiles, ClipboardFilesError, ClipboardIntent, PasteClipboardReply};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod unsupported;

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod arboard_text;

// The Windows Shell's file formats as bytes (`CF_HDROP`, `Preferred DropEffect`): pure, so every
// OS's tests pin them.
#[cfg(any(target_os = "windows", test))]
mod shell_formats;

const _: fn(AppHandle, Vec<PathBuf>, ClipboardIntent) -> Result<(), ClipboardFilesError> = write_file_urls;
const _: fn(AppHandle) -> Result<ClipboardFiles, ClipboardFilesError> = read_file_urls;
const _: fn(AppHandle) -> Result<Option<String>, String> = read_text;
const _: fn(Vec<PathBuf>) = after_cut_paste;
// An `async fn` doesn't coerce to a fn pointer, so this pin names its arguments and the output its
// future resolves to.
const _: fn(AppHandle) = |app| {
    fn resolves_to<T, F: Future<Output = T>>(_: F) {}
    resolves_to::<Result<PasteClipboardReply, MutationError>, _>(paste_as_file(app, None, String::new()));
};

/// Puts `paths` (native, plain form) on the clipboard as files for `intent`, and as their text,
/// one per line. Blocks until the OS answers, retrying a clipboard another app holds open, so
/// the command runs it off the async runtime (`spawn_blocking`).
pub(crate) use imp::write_file_urls;

/// The files on the clipboard, and whether it marks them cut. Blocks as [`write_file_urls`] does.
pub(crate) use imp::read_file_urls;

/// The plain text on the clipboard, or `None` when there's none. Blocks until the OS answers.
pub(crate) use imp::read_text;

/// A paste moved every file the clipboard marked cut (`clipboard::cut_paste`): tells the source
/// app the way it expects, and returns at once (the telling runs on its own thread). A no-op
/// where the clipboard carries no cut mark (macOS, Linux).
pub(crate) use imp::after_cut_paste;

/// Writes the clipboard's highest-intent non-file content (image, PDF, text) into `directory`
/// as a new `pasted.<ext>` file, under the instant-mutation reply deadline.
pub(crate) use imp::paste_as_file;
