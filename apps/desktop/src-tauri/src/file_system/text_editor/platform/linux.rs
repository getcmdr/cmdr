//! Linux: no editor choice yet (`text_editor_choice` is off), so the list is empty and F4 hands
//! the file to `xdg-open` (`commands/file_actions/platform/linux.rs`).

use super::super::TextEditorList;

/// Nothing listed, nothing chosen.
pub fn list_text_editors(_setting: &str) -> TextEditorList {
    TextEditorList::default()
}
