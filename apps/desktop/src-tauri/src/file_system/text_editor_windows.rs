//! The Windows half of `text_editor.rs`: the plain-text handlers the Shell lists (File
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

use std::path::Path;

use super::windows_rules::{WindowsEditor, chosen_id, resolve};
use super::{
    EditorOpenOutcome, EditorOpenReport, OpenInEditorError, SYSTEM_DEFAULT_CHOICE, TextEditorApp, TextEditorList,
};
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

fn is_program(path: &str) -> bool {
    Path::new(path).is_file()
}

/// A program's name for a row: its file name without `.exe`.
fn program_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |stem| stem.to_string_lossy().into_owned())
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
    let Some(apps) = crate::platform::shell::blocking_on_shell_thread("text-editors", || {
        assoc::apps_for(PLAIN_TEXT, Some(ICON_SIZE))
    }) else {
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
                    icon: crate::icons::get_icon_for_native(Path::new(id)),
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
    use crate::platform::shell::{ShellWait, on_shell_thread};
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
    let wait = on_shell_thread("open-in-editor", OPEN_TIMEOUT, move || {
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
    let apps = assoc::apps_for(PLAIN_TEXT, None);
    let (editor, outcome) = resolve(setting, &ids(&apps), is_program);
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
            match apps.iter().find(|app| app.is_default).or(apps.first()) {
                Some(app) => {
                    assoc::invoke(PLAIN_TEXT, &app.id, std::slice::from_ref(&paths.item)).map_err(refused)?;
                    Some(app.name.clone())
                }
                // Nothing claims plain text: the file's own default app, as a double-click does.
                None => {
                    crate::platform::shell::open_with_default_app(paths).map_err(refused)?;
                    None
                }
            }
        }
        WindowsEditor::Handler(id) => {
            assoc::invoke(PLAIN_TEXT, id, std::slice::from_ref(&paths.item)).map_err(refused)?;
            apps.iter().find(|app| &app.id == id).map(|app| app.name.clone())
        }
        WindowsEditor::Program(program) => {
            let mut command = std::process::Command::new(program);
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
