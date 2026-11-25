//! Windows: finds the four known terminals (`windows_table.rs`) and starts one in a
//! folder with `CreateProcessW`.
//!
//! - Installed-ness is a `symlink_metadata` probe of each executable's few known homes,
//!   ❌ never a directory scan. `symlink_metadata`, ❌ not `metadata`/`exists`: Windows
//!   Terminal's `wt.exe` is an app execution alias, a reparse point that following
//!   fails on.
//! - The folder is the process's working directory (`lpCurrentDirectory`), in its
//!   plain spelling (`AppPath::to_native_plain`): a working directory takes no `\\?\`
//!   form, and a folder with no faithful plain form is refused typed (`NoPlainPath`).
//! - `CreateProcessW` directly, ❌ not `std::process::Command`: `Command` hands the
//!   child Cmdr's own std handles whenever Cmdr has any (a dev run, a launch from a
//!   console), so a shell on a new console would read and write Cmdr's pipes instead of
//!   its window, and it lets every inheritable handle leak into the terminal. Here the
//!   child inherits no handles and its console sets up its own.

use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, NativeRefusal};

use super::super::{OpenTerminalError, OpenTerminalOutcome, TerminalApp, TerminalAppList, TerminalChoice};
use super::windows_table::{
    self, ExeHome, KNOWN_TERMINALS, LaunchPlan, Stored, WindowsChoice, WindowsTerminal, launch_plan,
};
use crate::platform::files::{is_file_or_alias, system_dir};

/// The probe's answer for one known terminal: where its executable is, if anywhere.
fn locate(terminal: &WindowsTerminal) -> Option<PathBuf> {
    terminal.homes.iter().find_map(|home| match home {
        ExeHome::Path => path_dirs()
            .into_iter()
            .map(|dir| dir.join(terminal.exe))
            .find(|p| is_file_or_alias(p)),
        _ => home_dir(*home)
            .map(|dir| dir.join(terminal.exe))
            .filter(|p| is_file_or_alias(p)),
    })
}

/// A fixed home's folder, or `None` when Windows doesn't say where it is.
fn home_dir(home: ExeHome) -> Option<PathBuf> {
    match home {
        ExeHome::WindowsApps => {
            std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join(r"Microsoft\WindowsApps"))
        }
        ExeHome::System(sub) => system_dir().map(|dir| if sub.is_empty() { dir } else { dir.join(sub) }),
        ExeHome::ProgramFiles(sub) => std::env::var_os("ProgramFiles").map(|base| PathBuf::from(base).join(sub)),
        ExeHome::Path => None,
    }
}

/// `PATH`'s absolute, local folders. A UNC entry is skipped: probing it can wait on a
/// server for as long as the network likes.
fn path_dirs() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute() && !dir.as_os_str().to_string_lossy().starts_with(r"\\"))
        .collect()
}

/// Where a choice's executable is, or `None` once it's gone.
fn choice_program(choice: &WindowsChoice) -> Option<PathBuf> {
    match choice {
        TerminalChoice::Known(known) => windows_table::known_terminal(known.id).and_then(locate),
        TerminalChoice::CustomApp(path) => is_file_or_alias(path).then(|| path.clone()),
    }
}

/// The dropdown entries for installed executables, `(id, name, program)` each. The icons are
/// the Shell's own for the files, as File Explorer shows them, asked together on the icon threads
/// (`icons::platform::native_icons`), never on this pool thread.
///
/// `is_running` is always `false`: the first-use picker adopts a terminal when exactly
/// one is running, and on Windows that reads nothing real. `cmd.exe` and
/// `powershell.exe` run hidden under build tools, installers, and scheduled tasks, and
/// Windows Terminal runs as `WindowsTerminal.exe`, not the `wt.exe` the table launches.
fn app_entries(found: Vec<(String, String, PathBuf)>) -> Vec<TerminalApp> {
    let icons = crate::icons::platform::native_icons(found.iter().map(|(_, _, program)| program.clone()).collect());
    found
        .into_iter()
        .zip(icons)
        .map(|((id, display_name, _), icon)| TerminalApp {
            id,
            display_name,
            icon,
            is_running: false,
        })
        .collect()
}

/// Probes each known terminal once, so the list, the chosen id, and the default all
/// read the same answer.
pub(crate) fn list_terminal_apps(setting: &str) -> TerminalAppList {
    let located: Vec<(&'static WindowsTerminal, PathBuf)> = KNOWN_TERMINALS
        .iter()
        .filter_map(|terminal| locate(terminal).map(|program| (terminal, program)))
        .collect();
    let mut found: Vec<(String, String, PathBuf)> = located
        .iter()
        .map(|(terminal, program)| {
            (
                terminal.known.id.to_string(),
                terminal.known.display_name.to_string(),
                program.clone(),
            )
        })
        .collect();

    let stored = windows_table::parse_choice(setting);
    // A custom pick isn't in the table, so it's appended as its own option;
    // otherwise the dropdown couldn't show what's currently selected.
    if let Stored::Choice(choice @ TerminalChoice::CustomApp(path)) = &stored
        && is_file_or_alias(path)
    {
        found.push((
            choice.id(),
            choice.name_without_io().unwrap_or_else(|| choice.id()),
            path.clone(),
        ));
    }
    let apps = app_entries(found);

    let installed = |choice: &WindowsChoice| match choice {
        TerminalChoice::Known(known) => located.iter().any(|(t, _)| t.known.id == known.id),
        TerminalChoice::CustomApp(_) => false,
    };
    let default_id = windows_table::default_choice(installed).id();
    let listed = |id: &String| apps.iter().any(|app| &app.id == id);
    let chosen_id = match &stored {
        Stored::Default => Some(default_id.clone()),
        Stored::Choice(choice) => Some(choice.id()),
        Stored::Unknown => None,
    }
    .filter(listed);
    let default_id = Some(default_id).filter(listed);
    TerminalAppList {
        apps,
        chosen_id,
        default_id,
    }
}

/// An id from the table, or a custom pick's file stem.
pub(crate) fn choice_display_name(setting: &str) -> Option<String> {
    windows_table::choice_display_name(setting)
}

/// Starts the chosen terminal in `dir`'s plain spelling.
pub(crate) fn open_in_terminal(dir: &AppPath, setting: &str) -> Result<OpenTerminalOutcome, OpenTerminalError> {
    let dir = match dir.to_native_plain() {
        Ok(dir) => dir,
        Err(NativeRefusal::NotPlainSafe(_)) => return Ok(OpenTerminalOutcome::NoPlainPath),
        Err(_) => return Ok(OpenTerminalOutcome::NotALocalPath),
    };
    let (choice, outcome) = windows_table::resolve(setting, |choice| choice_program(choice).is_some());
    let recipe = match &choice {
        TerminalChoice::Known(known) => Some(known.recipe),
        TerminalChoice::CustomApp(_) => None,
    };
    // The resolve above found it, so a `None` here is an uninstall in the last moment: refused,
    // ❌ never a bare name, which `CreateProcessW` would search for.
    let program = choice_program(&choice).ok_or(OpenTerminalError::LaunchRefused { errno: None })?;
    if windows_table::command_prompt_in_network_folder(&program, &dir) {
        return Ok(OpenTerminalOutcome::CommandPromptCantOpenNetworkFolder);
    }
    launch(&launch_plan(recipe, program, &dir))?;
    log::info!(
        target: "terminal",
        "opened {dir:?} in {} ({outcome:?})",
        choice.id(),
        dir = cmdr_fs::log_private::LogPrivate::dir_path(&dir),
    );
    Ok(outcome)
}

/// Starts the plan. Fire and forget: the process and thread handles are closed at once.
#[cfg(not(feature = "playwright-e2e"))]
fn launch(plan: &LaunchPlan) -> Result<(), OpenTerminalError> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_CONSOLE, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };

    /// NUL-terminated UTF-16 for a Win32 string parameter.
    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    let line = windows_table::command_line(plan).ok_or(OpenTerminalError::LaunchRefused { errno: None })?;
    let program = wide(plan.program.as_os_str());
    let mut line = wide(std::ffi::OsStr::new(&line));
    let cwd = wide(plan.cwd.as_os_str());
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    let flags = if plan.new_console { CREATE_NEW_CONSOLE } else { 0 };
    // SAFETY: every string is a live, NUL-terminated UTF-16 buffer for the whole call, and
    // `line` is mutable as `CreateProcessW` requires. No security attributes, no inherited
    // handles (`FALSE`), and the parent's environment (null). `startup` is a zeroed
    // `STARTUPINFOW` with `cb` set, and `info` is a writable `PROCESS_INFORMATION`.
    let created = unsafe {
        CreateProcessW(
            program.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            flags,
            std::ptr::null(),
            cwd.as_ptr(),
            &startup,
            &mut info,
        )
    };
    if created == 0 {
        // SAFETY: reads this thread's last-error value, set by the failed call above.
        let code = unsafe { GetLastError() };
        return Err(OpenTerminalError::LaunchRefused {
            errno: i32::try_from(code).ok(),
        });
    }
    // SAFETY: both handles came from the successful `CreateProcessW` above, are owned by
    // this call, and are closed exactly once; the processes run on without them.
    unsafe {
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
    Ok(())
}

/// E2E variant: record the folder instead of launching a terminal, so a suite run
/// doesn't pile up windows nothing can close. Same store as `open_path`, read back
/// through `e2e_opened_paths`; the FOLDER, as on macOS.
#[cfg(feature = "playwright-e2e")]
fn launch(plan: &LaunchPlan) -> Result<(), OpenTerminalError> {
    crate::open_mock::record(plan.cwd.to_string_lossy().into_owned());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_prompt_is_always_found() {
        let cmd = windows_table::known_terminal(windows_table::CMD_ID).expect("cmd is in the table");
        let program = locate(cmd).expect("every Windows carries cmd.exe in its system folder");
        assert!(program.ends_with("cmd.exe"));
    }

    #[test]
    fn windows_powershell_is_found_in_the_system_folder() {
        let ps = windows_table::known_terminal(windows_table::WINDOWS_POWERSHELL_ID).expect("in the table");
        assert!(locate(ps).is_some());
    }

    #[test]
    fn the_list_always_carries_a_default() {
        let list = list_terminal_apps("com.apple.Terminal");
        assert!(list.default_id.is_some());
        assert_eq!(list.chosen_id, list.default_id);
    }
}
