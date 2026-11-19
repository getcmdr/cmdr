//! The clipboard's plain text on Linux and Windows, through `arboard` (X11 and Wayland's data
//! control on Linux, `CF_UNICODETEXT` on Windows). The page's own Paste reads it: a text field's
//! Edit > Paste picked from an in-page bar or right-click menu (`text-field-menu.ts`), where the
//! engine's `execCommand('paste')` is refused.
//!
//! ❗ Called off the main thread (an async command's worker): on X11 our own process often owns
//! the selection (WebKitGTK put the text there), and GTK answers arboard's request from its main
//! loop, so a read ON that thread would wait for itself.

use tauri::AppHandle;

/// The plain text on the clipboard, or `None` when it holds none. Blocks until the OS answers.
pub(crate) fn read_text(_app: AppHandle) -> Result<Option<String>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Couldn't open the clipboard: {e}"))?;
    text_outcome(clipboard.get_text())
}

/// An `arboard` text read as the seam's answer: empty or non-text content is `None`, not a failure.
fn text_outcome(read: Result<String, arboard::Error>) -> Result<Option<String>, String> {
    match read {
        Ok(text) => Ok(Some(text)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(e) => Err(format!("Couldn't read the clipboard's text: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_the_clipboard_reads_as_itself() {
        assert_eq!(text_outcome(Ok("hello".to_string())), Ok(Some("hello".to_string())));
    }

    #[test]
    fn a_clipboard_without_text_reads_as_none() {
        assert_eq!(text_outcome(Err(arboard::Error::ContentNotAvailable)), Ok(None));
    }

    #[test]
    fn a_clipboard_that_refuses_reads_as_an_error() {
        assert!(text_outcome(Err(arboard::Error::ClipboardOccupied)).is_err());
    }
}
