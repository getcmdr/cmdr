use tauri::{AppHandle, Runtime};

use super::NativeEditSelector;

/// Nothing to run: no native bar exists here, so only a page's pick reaches an Edit row, and
/// `page_bar::pick` hands the edit back to the page first (`BarPickOutcome::TextEdit`). WebView2
/// has no editing command a host could send anyway.
pub(crate) fn run<R: Runtime>(_app: &AppHandle<R>, selector: NativeEditSelector) {
    log::debug!(target: "menu", "the {selector:?} edit is the page's own; nothing to run here");
}
