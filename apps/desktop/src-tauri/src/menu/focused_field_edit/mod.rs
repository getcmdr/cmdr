//! Running a Custom Edit item's text action (cut, copy, paste, select all) in the focused field of
//! a window other than the main one.
//!
//! The main bar's Edit items are Custom (`menu_handlers.rs` says why), so outside the main window
//! nothing does the text edit for them: macOS's native bar takes the accelerator before the page
//! sees the key, and the item's handler is the key's only road. macOS sends the selector down the
//! responder chain. Windows and Linux have no native bar: a page's pick of such a row hands the edit
//! back to the page (`page_bar::pick`, `BarPickOutcome::TextEdit`), the one place that can reach
//! its field, so their arms run nothing.

mod platform;

use super::menu_handlers::NativeEditSelector;
use tauri::{AppHandle, Runtime};

/// Runs `selector` in whichever window has focus, in its focused text field.
pub(crate) fn run_in_focused_window<R: Runtime>(app: &AppHandle<R>, selector: NativeEditSelector) {
    platform::run(app, selector);
}
