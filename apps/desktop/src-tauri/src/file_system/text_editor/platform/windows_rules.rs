//! Windows' F4 rules, pure: what the stored `behavior.textEditorApp` value names among the plain
//! text handlers the Shell lists, and what F4 does when that's gone. `windows.rs`
//! asks the Shell and launches; this decides.
//!
//! One string holds every shape, told apart without any I/O but the one `program_exists` answer:
//! the sentinel (`system`, or empty), a listed handler's id (`IAssocHandler::GetName`: a classic
//! app's program path or a packaged app's app id), or an absolute program path a "Choose an
//! app…" pick stored. A listed id wins over a path: a classic handler's id IS its program's path.

use super::super::{EditorOpenOutcome, SYSTEM_DEFAULT_CHOICE};

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
///
/// `listed` is `None` when the handler lookup broke: then nothing is known to be gone, so a
/// choice that isn't the default or an existing program is tried as a handler as it stands
/// (its launch says whether it's there), ❌ never reset to the default on a hiccup.
pub(crate) fn resolve(
    choice: &str,
    listed: Option<&[&str]>,
    program_exists: impl Fn(&str) -> bool,
) -> (WindowsEditor, EditorOpenOutcome) {
    match (named(choice, listed.unwrap_or(&[]), &program_exists), listed) {
        (Some(editor), _) => (editor, EditorOpenOutcome::Opened),
        (None, None) => (
            WindowsEditor::Handler(choice.trim().to_string()),
            EditorOpenOutcome::Opened,
        ),
        (None, Some(_)) => (
            WindowsEditor::SystemDefault,
            EditorOpenOutcome::ChosenAppMissingOpenedDefaultInstead,
        ),
    }
}

/// What "the system default" opens a file in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DefaultEditor {
    /// The plain-text handler with this id.
    Handler(String),
    /// Notepad, from the system folder, with the file as its one argument: what F4 falls back to
    /// when no handler claims plain text or the lookup broke. ❌ Never the file's own default
    /// verb: on a `setup.exe`, `x.bat`, or `x.ps1` that RUNS the file, and F4 must only open it.
    Notepad,
}

/// The system default among the plain-text handlers, each `(id, is_default)` (`None`: the lookup
/// broke): the marked default, else the Shell's first, else [`DefaultEditor::Notepad`].
pub(crate) fn default_editor(listed: Option<&[(&str, bool)]>) -> DefaultEditor {
    let listed = listed.unwrap_or(&[]);
    listed
        .iter()
        .find(|(_, is_default)| *is_default)
        .or_else(|| listed.first())
        .map_or(DefaultEditor::Notepad, |(id, _)| {
            DefaultEditor::Handler((*id).to_string())
        })
}

/// Notepad's program in the system folder `system_dir`, and its arguments for `file`: the file
/// alone, as one argument (`std::process::Command` quotes it), ❌ never a built command line.
pub(crate) fn notepad_plan(
    system_dir: &std::path::Path,
    file: &std::path::Path,
) -> (std::path::PathBuf, Vec<std::ffi::OsString>) {
    (system_dir.join("notepad.exe"), vec![file.as_os_str().to_owned()])
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
                resolve(choice, Some(LISTED), |_| false),
                (WindowsEditor::SystemDefault, EditorOpenOutcome::Opened)
            );
        }
    }

    #[test]
    fn a_listed_handler_is_found_by_its_id_ignoring_case() {
        assert_eq!(
            resolve(&NOTEPAD_PLUS.to_uppercase(), Some(LISTED), |_| false),
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
            resolve(picked, Some(LISTED), |path| path == picked),
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
                resolve(gone, Some(LISTED), |_| false),
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

    /// F4's default with nothing claiming plain text, or with the lookup broken, is Notepad from
    /// the system folder with the file as its argument: on a program or a script it opens the
    /// file's text and never runs it.
    #[test]
    fn with_no_plain_text_handler_the_default_is_notepad_never_the_files_own_verb() {
        for listed in [Some(&[][..]), None] {
            assert_eq!(default_editor(listed), DefaultEditor::Notepad);
        }
        let system = std::path::Path::new(r"C:\Windows\System32");
        for file in [r"D:\Downloads\setup.exe", r"D:\Scripts\x.bat"] {
            let file = std::path::Path::new(file);
            let (program, args) = notepad_plan(system, file);
            assert_eq!(program, system.join("notepad.exe"));
            assert_eq!(args, vec![file.as_os_str().to_owned()]);
        }
    }

    #[test]
    fn the_default_is_the_marked_handler_else_the_first() {
        assert_eq!(
            default_editor(Some(&[(NOTEPAD_PLUS, false), (NOTEPAD, true)])),
            DefaultEditor::Handler(NOTEPAD.to_string())
        );
        assert_eq!(
            default_editor(Some(&[(NOTEPAD_PLUS, false)])),
            DefaultEditor::Handler(NOTEPAD_PLUS.to_string())
        );
    }

    /// A broken lookup proves nothing gone: the stored handler is tried as it is, not reset.
    #[test]
    fn a_broken_lookup_keeps_the_stored_choice() {
        assert_eq!(
            resolve(NOTEPAD, None, |_| false),
            (WindowsEditor::Handler(NOTEPAD.to_string()), EditorOpenOutcome::Opened)
        );
        assert_eq!(resolve("system", None, |_| false).0, WindowsEditor::SystemDefault);
    }
}
