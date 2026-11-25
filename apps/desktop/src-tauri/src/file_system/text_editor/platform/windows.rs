//! The Windows half of `text_editor/mod.rs`: the plain-text handlers the Shell lists (File
//! Explorer's "Open with" for a `.txt`), and handing a file to the chosen one.
//!
//! - The list is `platform::assoc::apps_for("txt")`; its marked default is the "System default
//!   (…)" row, and F4's `system` choice opens the file in it whatever the file's own type, as
//!   macOS's `open -t` does.
//! - A listed handler gets the file through `IAssocHandler::Invoke`; a program picked by hand
//!   starts with the file as its one argument, in the file's folder. ❌ No command line is ever
//!   built by hand: `std::process::Command` quotes the argument.
//! - Which choice names what is `windows_rules.rs`, pure.
//!
//! `DETAILS.md` § "Text editor".

use std::path::{Path, PathBuf};

use super::super::{
    EditorOpenOutcome, EditorOpenReport, OpenInEditorError, SYSTEM_DEFAULT_CHOICE, TextEditorApp, TextEditorList,
};
use super::windows_rules::{DefaultEditor, WindowsEditor, chosen_id, default_editor, notepad_plan, resolve};
use crate::platform::assoc::{self, AssocApp};

/// The type whose handlers F4 offers.
const PLAIN_TEXT: &str = "txt";

/// The settings row's icon side, in pixels.
const ICON_SIZE: u32 = 32;

/// How long F4 waits for the Shell. Finding the handler is a registry read; a handler that
/// takes longer to start still opens the file, and the wait ends quietly (`TimedOut`).
const OPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The plain-text handlers' ids, in the Shell's spelling.
fn ids(apps: &[AssocApp]) -> Vec<&str> {
    apps.iter().map(|app| app.id.as_str()).collect()
}

/// Opens `file` in Notepad from the system folder, the file its one argument, in its own folder.
///
/// # Errors
///
/// `LaunchRefused` when the system folder has no Notepad or it won't start.
fn open_in_notepad(file: &Path) -> Result<(), OpenInEditorError> {
    let system = crate::platform::files::system_dir().ok_or(OpenInEditorError::LaunchRefused { errno: None })?;
    let (program, args) = notepad_plan(&system, file);
    if !crate::platform::files::is_file_or_alias(&program) {
        return Err(OpenInEditorError::LaunchRefused { errno: None });
    }
    let mut command = std::process::Command::new(program);
    command.args(args);
    if let Some(folder) = file.parent() {
        command.current_dir(folder);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| OpenInEditorError::LaunchRefused {
            errno: e.raw_os_error(),
        })
}

/// The stored choice is the native path the OS's file picker handed over ("Choose an app…"),
/// never app text, so it becomes a `PathBuf` as it is.
fn program_path(stored: &str) -> PathBuf {
    PathBuf::from(stored)
}

/// A picked editor that starts: a file, or an app execution alias (a packaged editor picked from
/// `%LOCALAPPDATA%\Microsoft\WindowsApps`), which `is_file` reads as gone.
fn is_program(stored: &str) -> bool {
    crate::platform::files::is_file_or_alias(&program_path(stored))
}

/// A program's name for a row: its file name without `.exe`.
fn program_name(stored: &str) -> String {
    program_path(stored)
        .file_stem()
        .map_or_else(|| stored.to_string(), |stem| stem.to_string_lossy().into_owned())
}

fn icon_url(app: &AssocApp) -> Option<String> {
    app.icon
        .as_ref()
        .and_then(|icon| crate::icons::rgba_to_data_url(icon.as_raw(), icon.width(), icon.height()))
}

fn row(app: &AssocApp) -> TextEditorApp {
    TextEditorApp {
        id: app.id.clone(),
        display_name: app.name.clone(),
        icon: icon_url(app),
    }
}

/// The plain-text editors Windows offers, the system default, and which one `setting` names.
/// Blocks on a Shell thread; the command bounds it.
pub fn list_text_editors(setting: &str) -> TextEditorList {
    let asked = crate::platform::shell::blocking_on_shell_thread("text-editors", || {
        assoc::apps_for(PLAIN_TEXT, Some(ICON_SIZE))
    });
    // No answer is no claim about which editors exist: an empty list with nothing chosen, which
    // the row shows as the default without writing anything.
    let Some(Ok(apps)) = asked else {
        log::warn!(target: "text_editor", "The plain-text handler lookup didn't answer: {asked:?}");
        return TextEditorList::default();
    };
    let default = apps.iter().find(|app| app.is_default);
    let chosen = chosen_id(setting, &ids(&apps), is_program);
    let mut rows: Vec<TextEditorApp> = apps.iter().filter(|app| !app.is_default).map(row).collect();
    if let Some(id) = &chosen {
        let listed = id == SYSTEM_DEFAULT_CHOICE || rows.iter().any(|app| &app.id == id);
        if !listed {
            // The default pinned by id gets a row of its own, as on macOS; so does a picked program.
            rows.push(match default.filter(|app| &app.id == id) {
                Some(app) => row(app),
                None => TextEditorApp {
                    id: id.clone(),
                    display_name: program_name(id),
                    icon: crate::icons::platform::native_icons(vec![program_path(id)])
                        .pop()
                        .flatten(),
                },
            });
        }
    }
    TextEditorList {
        default_app_name: default.map(|app| app.name.clone()),
        default_app_icon: default.and_then(icon_url),
        apps: rows,
        chosen_id: chosen,
    }
}

/// What a launch on the Shell thread came back with.
#[derive(Debug)]
struct Launched {
    outcome: EditorOpenOutcome,
    app_name: Option<String>,
    other_editors: bool,
}

/// F4 for the wire text `path`: the file in the editor `setting` names, or the system default
/// (and saying so) when that's gone.
///
/// # Errors
///
/// [`OpenInEditorError::NoPlainPath`] for a path an editor can't take, before anything launches;
/// `LaunchRefused` when the Shell or the program refused; `TimedOut` past [`OPEN_TIMEOUT`].
pub async fn open_in_editor(
    path: String,
    setting: String,
    ask_about_other_editors: bool,
) -> Result<EditorOpenReport, OpenInEditorError> {
    use crate::platform::shell::{HANDOFF_LINGER, ShellWait, on_shell_thread_lingering};
    use cmdr_fs::app_path::NativeRefusal;

    let app_path = crate::commands::wire_path::app_path(&path)
        .ok()
        .flatten()
        .ok_or(OpenInEditorError::LaunchRefused { errno: None })?;
    let paths = crate::platform::windows_shell_path::shell_launch_paths(&app_path).map_err(|why| match why {
        NativeRefusal::NotPlainSafe(_) => OpenInEditorError::NoPlainPath,
        _ => OpenInEditorError::LaunchRefused { errno: None },
    })?;
    let native = app_path
        .to_native_plain()
        .map_err(|_| OpenInEditorError::LaunchRefused { errno: None })?;
    // The thread stays a moment after `Invoke`, so a DDE or asynchronous handler gets the file.
    let wait = on_shell_thread_lingering("open-in-editor", OPEN_TIMEOUT, HANDOFF_LINGER, move || {
        launch(&paths, &native, &setting)
    })
    .await;
    let launched = match wait {
        ShellWait::Answered(answer) => answer?,
        ShellWait::TimedOut => return Err(OpenInEditorError::TimedOut),
        ShellWait::NoApartment(_) | ShellWait::NoThread => {
            return Err(OpenInEditorError::LaunchRefused { errno: None });
        }
    };
    let name_wanted = launched.outcome != EditorOpenOutcome::Opened || ask_about_other_editors;
    log::info!(
        target: "text_editor",
        "opened {} ({:?})",
        cmdr_fs::log_private::LogPrivate::path(&path),
        launched.outcome
    );
    Ok(EditorOpenReport {
        outcome: launched.outcome,
        opened_in_name: launched.app_name.filter(|_| name_wanted),
        other_editors_installed: ask_about_other_editors.then_some(launched.other_editors),
    })
}

/// Lists the handlers, resolves `setting`, and hands the file over. Runs in an apartment.
fn launch(
    paths: &crate::platform::windows_shell_path::ShellLaunchPaths,
    native: &Path,
    setting: &str,
) -> Result<Launched, OpenInEditorError> {
    // A broken lookup is told apart from "nothing claims plain text": neither may reset the
    // choice, and both fall back to Notepad, ❌ never the file's own verb.
    let asked = assoc::apps_for(PLAIN_TEXT, None);
    if let Err(refusal) = &asked {
        log::warn!(target: "text_editor", "The plain-text handler lookup broke: {refusal:?}");
    }
    let lookup_ok = asked.is_ok();
    let apps = asked.unwrap_or_default();
    let listed_ids = ids(&apps);
    let listed: Option<&[&str]> = lookup_ok.then_some(listed_ids.as_slice());
    let (editor, outcome) = resolve(setting, listed, is_program);
    let other_editors = apps.iter().any(|app| !app.is_default);
    let refused = |refusal: crate::platform::shell::ShellRefusal| {
        log::info!(target: "text_editor", "The Shell didn't open the file: {refusal:?}");
        OpenInEditorError::LaunchRefused {
            errno: match refusal {
                crate::platform::shell::ShellRefusal::Win32(code) => i32::try_from(code).ok(),
                _ => None,
            },
        }
    };
    let app_name = match &editor {
        WindowsEditor::SystemDefault => {
            let marked: Vec<(&str, bool)> = apps.iter().map(|app| (app.id.as_str(), app.is_default)).collect();
            match default_editor(lookup_ok.then_some(marked.as_slice())) {
                DefaultEditor::Handler(id) => {
                    assoc::invoke(PLAIN_TEXT, &id, std::slice::from_ref(&paths.item)).map_err(refused)?;
                    apps.iter().find(|app| app.id == id).map(|app| app.name.clone())
                }
                DefaultEditor::Notepad => {
                    open_in_notepad(native)?;
                    // Notepad's name is the system's, in its language; the toast words the nameless variant.
                    None
                }
            }
        }
        WindowsEditor::Handler(id) => {
            assoc::invoke(PLAIN_TEXT, id, std::slice::from_ref(&paths.item)).map_err(refused)?;
            apps.iter().find(|app| &app.id == id).map(|app| app.name.clone())
        }
        WindowsEditor::Program(program) => {
            let mut command = std::process::Command::new(program_path(program));
            command.arg(native);
            if let Some(folder) = native.parent() {
                command.current_dir(folder);
            }
            command.spawn().map_err(|e| OpenInEditorError::LaunchRefused {
                errno: e.raw_os_error(),
            })?;
            Some(program_name(program))
        }
    };
    Ok(Launched {
        outcome,
        app_name,
        other_editors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Windows edition opens plain text in something (Notepad), and the row names it.
    #[test]
    fn plain_text_lists_a_default_editor() {
        let list = list_text_editors(SYSTEM_DEFAULT_CHOICE);
        assert!(
            list.default_app_name.is_some() || !list.apps.is_empty(),
            "no plain-text handler at all: {list:?}"
        );
        assert_eq!(list.chosen_id.as_deref(), Some(SYSTEM_DEFAULT_CHOICE));
    }

    #[test]
    fn a_picked_program_gets_a_row_of_its_own() {
        let notepad = r"C:\Windows\System32\notepad.exe";
        let list = list_text_editors(notepad);
        let chosen = list.chosen_id.clone().expect("notepad.exe exists on every Windows");
        assert!(list.apps.iter().any(|app| app.id == chosen), "{list:?}");
    }

    #[test]
    fn a_gone_program_is_no_choice() {
        assert_eq!(list_text_editors(r"C:\Gone\editor.exe").chosen_id, None);
    }
}
