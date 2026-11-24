//! Windows' F4 rules, pure: what the stored `behavior.textEditorApp` value names among the plain
//! text handlers the Shell lists, and what F4 does when that's gone. `text_editor_windows.rs`
//! asks the Shell and launches; this decides.
//!
//! One string holds every shape, told apart without any I/O but the one `program_exists` answer:
//! the sentinel (`system`, or empty), a listed handler's id (`IAssocHandler::GetName`: a classic
//! app's program path or a packaged app's app id), or an absolute program path a "Choose an
//! app…" pick stored. A listed id wins over a path: a classic handler's id IS its program's path.

use super::{EditorOpenOutcome, SYSTEM_DEFAULT_CHOICE};

/// What F4 hands the file to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WindowsEditor {
    /// The plain-text default (the app `.txt` opens in), whatever the file's own type.
    SystemDefault,
    /// A listed handler, by its id in the Shell's own spelling.
    Handler(String),
    /// A program picked by hand, started with the file as its one argument.
    Program(String),
}

/// Whether `text` is an absolute Windows path: a drive (`C:\`, `C:/`) or a share (`\\host`).
fn is_absolute_windows_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/');
    drive || text.starts_with(r"\\")
}

/// What `choice` names right now, or `None` when it names something gone (a handler no longer
/// listed and no program at that path).
fn named(choice: &str, listed: &[&str], program_exists: &impl Fn(&str) -> bool) -> Option<WindowsEditor> {
    let choice = choice.trim();
    if choice.is_empty() || choice == SYSTEM_DEFAULT_CHOICE {
        return Some(WindowsEditor::SystemDefault);
    }
    if let Some(id) = listed.iter().find(|id| id.eq_ignore_ascii_case(choice)) {
        return Some(WindowsEditor::Handler((*id).to_string()));
    }
    (is_absolute_windows_path(choice) && program_exists(choice)).then(|| WindowsEditor::Program(choice.to_string()))
}

/// What F4 hands the file to for the stored `choice`, and the outcome to report: a choice that
/// names nothing any more falls back to the system default and says so.
pub(crate) fn resolve(
    choice: &str,
    listed: &[&str],
    program_exists: impl Fn(&str) -> bool,
) -> (WindowsEditor, EditorOpenOutcome) {
    match named(choice, listed, &program_exists) {
        Some(editor) => (editor, EditorOpenOutcome::Opened),
        None => (
            WindowsEditor::SystemDefault,
            EditorOpenOutcome::ChosenAppMissingOpenedDefaultInstead,
        ),
    }
}

/// The stored `choice` in canonical form, for the settings row: `system`, a listed handler's id
/// in the Shell's spelling (a pick of a listed program by path becomes that id), or the picked
/// program's path. `None` once it names nothing.
pub(crate) fn chosen_id(choice: &str, listed: &[&str], program_exists: impl Fn(&str) -> bool) -> Option<String> {
    named(choice, listed, &program_exists).map(|editor| match editor {
        WindowsEditor::SystemDefault => SYSTEM_DEFAULT_CHOICE.to_string(),
        WindowsEditor::Handler(id) | WindowsEditor::Program(id) => id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTEPAD_PLUS: &str = r"C:\Program Files\Notepad++\notepad++.exe";
    const NOTEPAD: &str = "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App";
    const LISTED: &[&str] = &[NOTEPAD_PLUS, NOTEPAD];

    #[test]
    fn the_sentinel_and_an_empty_setting_are_the_system_default() {
        for choice in ["system", "", "  "] {
            assert_eq!(
                resolve(choice, LISTED, |_| false),
                (WindowsEditor::SystemDefault, EditorOpenOutcome::Opened)
            );
        }
    }

    #[test]
    fn a_listed_handler_is_found_by_its_id_ignoring_case() {
        assert_eq!(
            resolve(&NOTEPAD_PLUS.to_uppercase(), LISTED, |_| false),
            (
                WindowsEditor::Handler(NOTEPAD_PLUS.to_string()),
                EditorOpenOutcome::Opened
            )
        );
        assert_eq!(
            chosen_id(&NOTEPAD_PLUS.to_lowercase(), LISTED, |_| true).as_deref(),
            Some(NOTEPAD_PLUS)
        );
    }

    #[test]
    fn a_picked_program_that_exists_is_started_by_its_path() {
        let picked = r"D:\Tools\micro\micro.exe";
        assert_eq!(
            resolve(picked, LISTED, |path| path == picked),
            (WindowsEditor::Program(picked.to_string()), EditorOpenOutcome::Opened)
        );
        assert_eq!(chosen_id(picked, LISTED, |_| true).as_deref(), Some(picked));
        let on_a_share = r"\\testnas.local\tools\edit.exe";
        assert_eq!(chosen_id(on_a_share, LISTED, |_| true).as_deref(), Some(on_a_share));
    }

    /// An uninstalled handler, a deleted program, and a macOS bundle id synced from a Mac all
    /// land on the default, and say so.
    #[test]
    fn a_choice_that_names_nothing_falls_back_to_the_default_and_says_so() {
        for gone in [
            "Some.Uninstalled_App!App",
            r"C:\Gone\editor.exe",
            "com.microsoft.VSCode",
            "relative\\x.exe",
        ] {
            assert_eq!(
                resolve(gone, LISTED, |_| false),
                (
                    WindowsEditor::SystemDefault,
                    EditorOpenOutcome::ChosenAppMissingOpenedDefaultInstead
                ),
                "{gone}"
            );
            assert_eq!(chosen_id(gone, LISTED, |_| false), None, "{gone}");
        }
        // A relative path is never a program, even when something answers for it.
        assert_eq!(chosen_id("relative\\x.exe", LISTED, |_| true), None);
    }
}
