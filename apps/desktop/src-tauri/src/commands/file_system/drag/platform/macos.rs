//! macOS: an `NSDraggingSession` built on the main thread (`crate::native_drag`), a file-url or a
//! file-promise layout by the session's locality, and the self-drag image swap
//! (`crate::drag_image_swap`).

use super::locality::{SessionLocality, session_locality};
use crate::native_drag::{self, DragSessionLocality};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use tauri::Manager;

/// Builds the session for `paths` on the main thread.
pub(crate) fn start_drag(
    app: tauri::AppHandle,
    paths: Vec<String>,
    icon_path: String,
    source_volume_id: Option<String>,
) -> Result<(), String> {
    // Rows as sent, for the lexical route check and a promise session, which `native_drag` still
    // reads back through the volume by `std::path` text (#1310).
    let sent: Vec<PathBuf> = paths
        .iter()
        .map(|row| crate::commands::wire_path::as_sent_std_path(row))
        .collect();
    if sent.is_empty() {
        return Err("No valid files to drag".to_string());
    }
    let locality = match session_locality(&sent, source_volume_id.as_deref()) {
        SessionLocality::Local => DragSessionLocality::Local,
        SessionLocality::Virtual => DragSessionLocality::Virtual,
    };
    // A local session hands the OS file URLs, so each row crosses to its native file.
    let path_bufs = match locality {
        DragSessionLocality::Local => paths.iter().filter_map(|row| crate::app_text::native(row)).collect(),
        DragSessionLocality::Virtual => sent,
    };
    if path_bufs.is_empty() {
        return Err("No valid files to drag".to_string());
    }
    // The drag image the frontend wrote into the OS temp folder: native text.
    let icon = crate::native_text::os_path_buf(icon_path);
    run_drag_on_main_thread(&app, path_bufs, icon, locality, source_volume_id)
}

/// Hops to the AppKit main thread, builds the drag session, and returns synchronously.
/// `NSDraggingItem`s and the source class are not `Send`, so everything happens inside
/// the closure; the result travels back via a one-shot channel.
fn run_drag_on_main_thread(
    app: &tauri::AppHandle,
    paths: Vec<PathBuf>,
    icon_path: PathBuf,
    locality: DragSessionLocality,
    source_volume_id: Option<String>,
) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("Main window not found")?;
    let (tx, rx) = channel();

    app.run_on_main_thread(move || {
        let result = native_drag::start_drag(&window, paths, &icon_path, locality, source_volume_id.as_deref());
        let _ = tx.send(result);
    })
    .map_err(|e| format!("Failed to run on main thread: {}", e))?;

    rx.recv().map_err(|_| "Failed to receive drag result")?
}

/// Nothing to install: the swizzle on wry's own view (`drag_image_detection.rs`) does this job.
pub(crate) fn install_drop_target(_webview: &tauri::Webview<tauri::Wry>) {}

/// Marks the self-drag active with its rich image.
pub(crate) fn prepare_self_drag_overlay(rich_image_path: String) {
    crate::drag_image_swap::set_self_drag_active(rich_image_path);
}

/// Clears the self-drag state.
pub(crate) fn clear_self_drag_overlay() {
    crate::drag_image_swap::clear_self_drag_state();
}

/// Pushes the resolved operation to the swizzle; unknown values are ignored.
pub(crate) fn set_self_drag_resolved_op(operation: String) {
    use crate::drag_image_swap::SelfDragOp;
    let op = match operation.as_str() {
        "copy" => SelfDragOp::Copy,
        "move" => SelfDragOp::Move,
        _ => return,
    };
    crate::drag_image_swap::set_self_drag_resolved_op(op);
}
