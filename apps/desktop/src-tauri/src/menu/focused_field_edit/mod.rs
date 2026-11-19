//! Running a Custom Edit item's text action (cut, copy, paste, select all) in the focused field of
//! a window other than the main one, on macOS.
//!
//! The main bar's Edit items are Custom (`menu_handlers.rs` says why), so outside the main window
//! nothing does the text edit for them: the native bar takes the accelerator before the page sees
//! the key, and the item's handler is the key's only road. macOS sends the selector down the
//! responder chain. Off macOS no native bar exists: a page's pick of such a row hands the edit back
//! to the page (`page_bar::pick`, `BarPickOutcome::TextEdit`), the one place that can reach its
//! field (WebView2 has no editing command a host can send).

use super::menu_handlers::NativeEditSelector;
use tauri::{AppHandle, Runtime};

/// Runs `selector` in the key window's focused text field. The responder chain finds the key
/// window's first responder, so the window needs no lookup. The menu handler runs on the main
/// thread, as AppKit requires.
pub(crate) fn run_in_focused_window<R: Runtime>(_app: &AppHandle<R>, selector: NativeEditSelector) {
    super::menu_handlers::send_native_edit_selector(selector);
}
