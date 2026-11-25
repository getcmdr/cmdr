//! Tauri commands for clipboard file operations (copy/cut/paste).

use std::collections::HashSet;
use std::path::PathBuf;

use crate::clipboard::{self, ClipboardFiles, ClipboardFilesError, ClipboardIntent};
use cmdr_fs::app_path::NativeRefusal;
use cmdr_operations::MutationError;

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardReadResult {
    paths: Vec<String>,
    is_cut: bool,
    /// Per-path top-level kind, index-aligned with `paths`: `Some(true)` =
    /// directory, `Some(false)` = file, `None` = unknown (stat failed). Lets the
    /// paste path split the completion toast into files vs. folders without
    /// walking trees. Clipboard file URLs are always real local paths, so in
    /// practice these resolve; `None` falls back to the flattened wording.
    is_directory: Vec<Option<bool>>,
}

/// The rows as the clipboard names them, each crossed at the edge to its PLAIN native path (no
/// `\\?\` on Windows: other apps read `CF_HDROP` as plain paths; macOS and Linux have one form).
/// A row with no native form (a phone, a server path, a `..`) refuses the WHOLE set, as the
/// frontend's own gate does (`clipboard-operations.ts`): a partial copy would put a subset on the
/// clipboard under a toast saying the copy happened. So does a row whose plain form would name a
/// different file (Windows: a long path, a trailing dot, `CON`), naming it.
fn native_rows(rows: &[String]) -> Result<Vec<PathBuf>, ClipboardFilesError> {
    if rows.is_empty() {
        return Err(ClipboardFilesError::NothingToCopy);
    }
    rows.iter().map(|row| native_row(row)).collect()
}

fn native_row(row: &str) -> Result<PathBuf, ClipboardFilesError> {
    let Ok(Some(path)) = crate::commands::wire_path::app_path(row) else {
        return Err(ClipboardFilesError::NothingToCopy);
    };
    match path.to_native_plain() {
        Ok(native) => Ok(native),
        Err(NativeRefusal::NotPlainSafe(_)) => Err(ClipboardFilesError::NoPlainPath { path: row.to_string() }),
        Err(_) => Err(ClipboardFilesError::NothingToCopy),
    }
}

/// The app paths the frontend works in for the native paths the clipboard held (`C:\x` is
/// `C:/x`), through `read` (the OS arm's `app_text`). One that names no file for certain refuses
/// the WHOLE paste, naming it: pasting the rest would move or copy a subset under a toast saying
/// the paste happened.
fn app_texts(
    natives: &[PathBuf],
    read: impl Fn(&std::path::Path) -> Option<String>,
) -> Result<Vec<String>, ClipboardFilesError> {
    natives
        .iter()
        .map(|native| {
            read(native).ok_or_else(|| {
                log::warn!(
                    target: "clipboard",
                    "Refused a paste: a path names no file for certain: {}",
                    cmdr_fs::log_private::LogPrivate::path(native)
                );
                ClipboardFilesError::UnreadablePath {
                    path: native.to_string_lossy().into_owned(),
                }
            })
        })
        .collect()
}

/// A cut that didn't reach the clipboard leaves no earlier cut armed: Cmdr's cut state goes, and
/// so does the clipboard's move mark while it still holds that earlier cut of Cmdr's, so the next
/// paste copies (or finds the clipboard empty) instead of moving files the person moved on from.
async fn disarm_earlier_cut() {
    let Some(earlier) = clipboard::get_cut_state() else {
        return;
    };
    clipboard::clear_cut_state();
    if let Err(error) = off_runtime(move || clipboard::platform::drop_cut_mark(earlier)).await {
        log::warn!(target: "clipboard", "Couldn't drop the earlier cut's mark: {error:?}");
    }
}

/// Runs a blocking clipboard call off the async runtime: the OS answers at once, or after the
/// arm's bounded retries while another app holds the clipboard.
async fn off_runtime<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, ClipboardFilesError> + Send + 'static,
) -> Result<T, ClipboardFilesError> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|e| ClipboardFilesError::Unexpected {
            detail: format!("the clipboard task didn't finish: {e}"),
        })?
}

/// Writes `rows` to the clipboard for `intent`, answering how many went.
async fn write_rows(
    app: tauri::AppHandle,
    rows: &[String],
    intent: ClipboardIntent,
) -> Result<usize, ClipboardFilesError> {
    let written = match native_rows(rows) {
        Ok(paths) => {
            let to_write = paths.clone();
            off_runtime(move || clipboard::platform::write_file_urls(app, to_write, intent))
                .await
                .map(|()| paths)
        }
        Err(refused) => Err(refused),
    };
    let paths = match written {
        Ok(paths) => paths,
        Err(refused) => {
            if intent == ClipboardIntent::Cut {
                disarm_earlier_cut().await;
            }
            return Err(refused);
        }
    };
    let count = paths.len();
    let cut_paths = paths.clone();
    match intent {
        ClipboardIntent::Copy => clipboard::clear_cut_state(),
        ClipboardIntent::Cut => clipboard::set_cut_state(cut_paths),
    }
    Ok(count)
}

/// Writes the given paths to the system clipboard and clears any cut state.
///
/// Paths by value, never row numbers: the frontend resolves a pane's selection
/// through the revision-checked `get_selection_snapshot` first, because a watcher
/// insert renumbers rows and old row numbers would name the neighbor.
#[tauri::command]
#[specta::specta]
pub async fn copy_paths_to_clipboard(app: tauri::AppHandle, paths: Vec<String>) -> Result<usize, ClipboardFilesError> {
    write_rows(app, &paths, ClipboardIntent::Copy).await
}

/// Writes the given paths directly to the system clipboard and marks them as cut.
#[tauri::command]
#[specta::specta]
pub async fn cut_paths_to_clipboard(app: tauri::AppHandle, paths: Vec<String>) -> Result<usize, ClipboardFilesError> {
    write_rows(app, &paths, ClipboardIntent::Cut).await
}

/// Reads file URLs from the system clipboard and checks whether they were cut: marked so by the
/// clipboard itself (Windows' `Preferred DropEffect`, an Explorer Ctrl+X), or by Cmdr's own cut
/// state.
///
/// If the clipboard contents no longer match the recorded cut state (the user copied
/// something else), the stale cut state is automatically cleared.
#[tauri::command]
#[specta::specta]
pub async fn read_clipboard_files(app: tauri::AppHandle) -> Result<ClipboardReadResult, ClipboardFilesError> {
    read_for_paste(move || clipboard::platform::read_file_urls(app)).await
}

/// [`read_clipboard_files`] over an injected clipboard read.
async fn read_for_paste(
    read: impl FnOnce() -> Result<ClipboardFiles, ClipboardFilesError> + Send + 'static,
) -> Result<ClipboardReadResult, ClipboardFilesError> {
    let ClipboardFiles {
        paths: clipboard_paths,
        marked_cut,
    } = off_runtime(read).await?;

    // Check cut state: if set, verify paths match (order-insensitive)
    let cut_by_cmdr = if let Some(cut_paths) = clipboard::get_cut_state() {
        let clipboard_set: HashSet<&PathBuf> = clipboard_paths.iter().collect();
        let cut_set: HashSet<&PathBuf> = cut_paths.iter().collect();

        if clipboard_set == cut_set {
            true
        } else {
            // Clipboard changed since the cut -- clear stale state
            clipboard::clear_cut_state();
            false
        }
    } else {
        false
    };

    if marked_cut && every_source_is_gone(clipboard_paths.clone()).await {
        return Ok(used_up_cut(clipboard_paths, cut_by_cmdr).await);
    }
    let is_cut = marked_cut || cut_by_cmdr;

    let paths = app_texts(&clipboard_paths, clipboard::platform::app_text)?;
    // A cut the clipboard marks owes its source a word once a move takes it all.
    clipboard::cut_paste::note_read(&paths, &clipboard_paths, marked_cut);

    // Resolve each path's top-level kind so the paste completion toast can split
    // files vs. folders. One batched stat off the main thread (the pasteboard
    // read already happened above); per-item failures map to `None`, never an
    // error for the batch. Clipboard URLs are real local paths, so this is fast.
    let paths_for_stat = paths.clone();
    let is_directory =
        tokio::task::spawn_blocking(move || crate::commands::file_system::stat_paths_kinds_blocking(&paths_for_stat))
            .await
            .unwrap_or_else(|_| vec![None; paths.len()]);

    Ok(ClipboardReadResult {
        paths,
        is_cut,
        is_directory,
    })
}

/// Whether none of a cut's files is where the clipboard names it: another app already pasted the
/// cut (Explorer moves a Cmdr cut, and with no data object to answer, nothing tells Cmdr; #1478).
/// Only a sure "not found" counts as gone: a file Cmdr can't stat still pastes, and fails typed.
async fn every_source_is_gone(natives: Vec<PathBuf>) -> bool {
    tokio::task::spawn_blocking(move || {
        !natives.is_empty()
            && natives.iter().all(|native| {
                std::fs::symlink_metadata(native).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            })
    })
    .await
    .unwrap_or(false)
}

/// A cut someone else already pasted is used up, as a cut pasted in Explorer is: the paste finds
/// no files (the frontend's quiet "No files on the clipboard"), ❌ never a move that fails on files
/// that are gone. Cmdr's own cut also leaves the clipboard, so the next Ctrl+V finds it empty
/// too; another app's cut stays that app's to end.
async fn used_up_cut(natives: Vec<PathBuf>, cut_by_cmdr: bool) -> ClipboardReadResult {
    log::info!(
        target: "clipboard",
        "The clipboard's cut of {} item(s) was already pasted elsewhere: its files are gone",
        natives.len()
    );
    clipboard::cut_paste::note_read(&[], &[], false);
    if cut_by_cmdr {
        clipboard::clear_cut_state();
        if let Err(error) = off_runtime(move || clipboard::platform::drop_cut_mark(natives)).await {
            log::warn!(target: "clipboard", "Couldn't empty the used-up cut off the clipboard: {error:?}");
        }
    }
    ClipboardReadResult {
        paths: Vec::new(),
        is_cut: false,
        is_directory: Vec::new(),
    }
}

/// Reads plain text from the system clipboard.
///
/// Used by the frontend to paste text into input fields. Going through Rust bypasses
/// WebKit's `navigator.clipboard.readText()` permission popup.
#[tauri::command]
#[specta::specta]
pub async fn read_clipboard_text(app: tauri::AppHandle) -> Result<Option<String>, String> {
    clipboard::platform::read_text(app)
}

/// Reads the highest-intent non-file clipboard flavor (image / PDF / text) and
/// writes it into `directory` as a new `pasted.<ext>` file. Answers
/// `Done { file }` with the created file's name + kind (`file: None` = nothing
/// pasteable, the typed no-op the frontend treats as "no file created", NOT an
/// error), or `StillRunning` past the reply deadline with the real end on
/// `clipboard-paste-settled`.
///
/// Thin edge: reads the RAW pasteboard flavors on the main thread (NSPasteboard is
/// main-thread-only), then picks the flavor + converts TIFF→PNG OFF the main
/// thread (that decode can be hundreds of ms — never on the UI thread), and hands
/// the result to `clipboard::paste_file::write_payload_replying`.
/// `directory` is tilde-expanded for the local `root` volume only.
#[tauri::command]
#[specta::specta]
pub async fn paste_clipboard_as_file(
    app: tauri::AppHandle,
    volume_id: Option<String>,
    directory: String,
) -> Result<clipboard::PasteClipboardReply, MutationError> {
    clipboard::platform::paste_as_file(app, volume_id, directory).await
}

/// Clears the in-process cut state without touching the system clipboard.
#[tauri::command]
#[specta::specta]
pub fn clear_clipboard_cut_state() {
    clipboard::clear_cut_state();
}

#[cfg(test)]
mod tests {
    use super::{ClipboardFiles, ClipboardFilesError, app_texts, disarm_earlier_cut, native_rows, read_for_paste};

    /// Every row crosses, in order; one row with no native file (a server's, a `..`) refuses the
    /// whole set, ❌ never a quiet subset under a "copied" toast.
    #[test]
    fn the_rows_cross_together_or_not_at_all() {
        let dir = crate::test_support::AppTestDir::new("clipboard_rows");
        let a = format!("{}/a.txt", dir.app_path().as_str());
        let b = format!("{}/b.txt", dir.app_path().as_str());
        let crossed = native_rows(&[a.clone(), b.clone()]).expect("both cross");
        assert_eq!(crossed.len(), 2);
        assert!(crossed[0].ends_with("a.txt") && crossed[1].ends_with("b.txt"));
        assert_eq!(
            native_rows(&[a.clone(), "sftp://testnas.local:22/x".to_string()]),
            Err(ClipboardFilesError::NothingToCopy)
        );
        assert_eq!(
            native_rows(&[a, format!("{b}/../c.txt")]),
            Err(ClipboardFilesError::NothingToCopy)
        );
        assert_eq!(native_rows(&[]), Err(ClipboardFilesError::NothingToCopy));
    }

    /// Windows: a row whose plain form names another file (Win32 strips a trailing dot) refuses
    /// the set by name: Explorer would read `CF_HDROP`'s `dup.` as `dup`.
    #[cfg(windows)]
    #[test]
    fn a_row_with_no_plain_form_refuses_the_set_by_name() {
        let dir = crate::test_support::AppTestDir::new("clipboard_plain");
        let fine = format!("{}/a.txt", dir.app_path().as_str());
        let dotted = format!("{}/dup.", dir.app_path().as_str());
        assert_eq!(
            native_rows(&[fine, dotted.clone()]),
            Err(ClipboardFilesError::NoPlainPath { path: dotted })
        );
    }

    /// A native path off the clipboard reads back as the app path the panes speak, so the move
    /// it starts names the same path the cut ledger noted.
    #[test]
    fn a_native_clipboard_path_reads_as_its_app_path() {
        let dir = crate::test_support::AppTestDir::new("clipboard_app_text");
        let app = format!("{}/a.txt", dir.app_path().as_str());
        let native = dir.app_path().to_native().expect("native").join("a.txt");
        assert_eq!(
            app_texts(std::slice::from_ref(&native), crate::clipboard::platform::app_text),
            Ok(vec![app])
        );
    }

    /// One path that names no file for certain refuses the whole paste, naming it: ❌ never a
    /// silent subset.
    #[test]
    fn one_unreadable_path_refuses_the_whole_paste() {
        let dir = crate::test_support::AppTestDir::new("clipboard_unreadable");
        let native = dir.app_path().to_native().expect("native");
        let fine = native.join("a.txt");
        let odd = native.join("odd");
        let read = |p: &std::path::Path| (!p.ends_with("odd")).then(|| p.to_string_lossy().into_owned());
        assert_eq!(
            app_texts(&[fine, odd.clone()], read),
            Err(ClipboardFilesError::UnreadablePath {
                path: odd.to_string_lossy().into_owned()
            })
        );
    }

    /// A refused cut leaves no earlier cut armed: the next paste mustn't move the files cut
    /// before it.
    #[tokio::test]
    async fn a_refused_cut_disarms_the_earlier_cut() {
        let dir = crate::test_support::AppTestDir::new("clipboard_disarm");
        crate::clipboard::set_cut_state(vec![dir.app_path().to_native().expect("native").join("earlier.txt")]);
        disarm_earlier_cut().await;
        assert_eq!(crate::clipboard::get_cut_state(), None);
    }

    /// A cut another app already pasted (every file gone from where the clipboard names it) is
    /// used up: the paste finds no files and Cmdr's cut state goes, ❌ never a move that fails on
    /// files that are gone (#1478).
    #[tokio::test]
    async fn a_cut_whose_files_are_all_gone_reads_as_no_files() {
        let dir = crate::test_support::AppTestDir::new("clipboard_used_up");
        let native = dir.app_path().to_native().expect("native");
        let gone = vec![native.join("x1.txt"), native.join("x2.txt")];
        crate::clipboard::set_cut_state(gone.clone());
        let read = read_for_paste(move || {
            Ok(ClipboardFiles {
                paths: gone,
                marked_cut: true,
            })
        })
        .await
        .expect("a read");
        assert!(read.paths.is_empty(), "got {:?}", read.paths);
        assert!(!read.is_cut);
        assert_eq!(crate::clipboard::get_cut_state(), None);
    }

    /// A cut with one file still there pastes as a move, as before: only a cut wholly gone is
    /// used up.
    #[tokio::test]
    async fn a_cut_with_a_file_still_there_still_moves() {
        let dir = crate::test_support::AppTestDir::new("clipboard_cut_left");
        let native = dir.app_path().to_native().expect("native");
        std::fs::write(native.join("here.txt"), b"x").expect("a source");
        let paths = vec![native.join("here.txt"), native.join("gone.txt")];
        let read = read_for_paste(move || {
            Ok(ClipboardFiles {
                paths,
                marked_cut: true,
            })
        })
        .await
        .expect("a read");
        assert_eq!(read.paths.len(), 2);
        assert!(read.is_cut);
    }
}
