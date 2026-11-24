//! Windows: the known-terminals table, how a stored setting reads, and the pure
//! launch plan each choice turns into. No I/O and no Win32, so it compiles into every
//! OS's test build and its tests run on a Mac too; `platform/windows.rs` does the
//! probing and the `CreateProcessW`.
//!
//! - [`KNOWN_TERMINALS`]: Windows Terminal, PowerShell 7, Windows PowerShell, and
//!   Command Prompt, in the order the default prefers them.
//! - [`resolve`]: a setting plus "is this installed?" → the app to launch.
//! - [`launch_plan`] and [`command_line`]: a choice + folder → program, arguments,
//!   working directory, and console flag. ❌ Never a shell string: the folder goes in
//!   as the process's working directory, never into its command line.

use std::path::{Path, PathBuf};

use super::super::{KnownTerminal, OpenTerminalOutcome, TerminalChoice};

/// How one Windows terminal takes the folder it should start in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowsRecipe {
    /// `wt.exe -d .`, started IN the folder. Windows Terminal splits its whole command
    /// line on `;` into subcommands, so the folder never goes into its argv: `.` is
    /// resolved against the working directory Cmdr hands it.
    WindowsTerminal,
    /// A console shell (`pwsh`, `powershell`, `cmd`) started in the folder with no
    /// arguments, on a console of its own: Cmdr is a GUI process, and from a dev run it
    /// has a console the shell would otherwise share.
    ConsoleShell,
}

/// Where a known terminal's executable lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExeHome {
    /// `%LOCALAPPDATA%\Microsoft\WindowsApps`, where a Store app's execution alias
    /// sits (a reparse point, so it's probed with `symlink_metadata`).
    WindowsApps,
    /// The system folder (`GetSystemDirectoryW`, `C:\Windows\System32`), joined with a
    /// relative subfolder (empty for the folder itself).
    System(&'static str),
    /// `%ProgramFiles%`, joined with a relative subfolder.
    ProgramFiles(&'static str),
    /// Every absolute, local folder on `PATH`, in order.
    Path,
}

/// One Windows terminal: its table entry plus where to find its executable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowsTerminal {
    /// The table entry: stable id, display name, recipe.
    pub(crate) known: KnownTerminal<WindowsRecipe>,
    /// The executable's file name.
    pub(crate) exe: &'static str,
    /// Where to look, first hit wins. A fixed folder before `PATH`, so the common case
    /// costs one probe and a `PATH` entry can't shadow the system's own shell.
    pub(crate) homes: &'static [ExeHome],
}

/// Windows Terminal's id. The ids are what the setting stores, so they never change.
pub(crate) const WINDOWS_TERMINAL_ID: &str = "windows-terminal";
/// PowerShell 7's id.
pub(crate) const PWSH_ID: &str = "pwsh";
/// Windows PowerShell's id (5.1, shipped with Windows).
pub(crate) const WINDOWS_POWERSHELL_ID: &str = "windows-powershell";
/// Command Prompt's id.
pub(crate) const CMD_ID: &str = "cmd";

/// The setting's registry default (`behavior.openTerminalHereApp`), shared with
/// macOS, where it names Terminal.app. On Windows it reads as "the default", ❌ never
/// as an app that went missing, so a fresh install raises no "isn't installed" toast.
pub(crate) const REGISTRY_DEFAULT: &str = "com.apple.Terminal";

/// Every Windows terminal Cmdr knows, in the order the default prefers them (which is
/// also the dropdown order): the modern host first, then the newest shell, then the
/// two that ship with every Windows. Locations verified against Microsoft's docs,
/// 2026-11-24: Windows Terminal's `wt.exe` alias (learn.microsoft.com
/// `windows/terminal/command-line-arguments`), PowerShell 7's MSI folder and Store
/// alias (`powershell/scripting/install/installing-powershell-on-windows`).
pub(crate) const KNOWN_TERMINALS: &[WindowsTerminal] = &[
    WindowsTerminal {
        known: KnownTerminal {
            id: WINDOWS_TERMINAL_ID,
            display_name: "Windows Terminal",
            recipe: WindowsRecipe::WindowsTerminal,
        },
        exe: "wt.exe",
        homes: &[ExeHome::WindowsApps, ExeHome::Path],
    },
    WindowsTerminal {
        known: KnownTerminal {
            id: PWSH_ID,
            display_name: "PowerShell",
            recipe: WindowsRecipe::ConsoleShell,
        },
        exe: "pwsh.exe",
        homes: &[
            ExeHome::ProgramFiles(r"PowerShell\7"),
            ExeHome::WindowsApps,
            ExeHome::Path,
        ],
    },
    WindowsTerminal {
        known: KnownTerminal {
            id: WINDOWS_POWERSHELL_ID,
            display_name: "Windows PowerShell",
            recipe: WindowsRecipe::ConsoleShell,
        },
        exe: "powershell.exe",
        homes: &[ExeHome::System(r"WindowsPowerShell\v1.0")],
    },
    WindowsTerminal {
        known: KnownTerminal {
            id: CMD_ID,
            display_name: "Command Prompt",
            recipe: WindowsRecipe::ConsoleShell,
        },
        exe: "cmd.exe",
        homes: &[ExeHome::System("")],
    },
];

/// A Windows choice: a known terminal, or a custom `.exe` path.
pub(crate) type WindowsChoice = TerminalChoice<WindowsRecipe>;

/// The table row for a known choice id.
pub(crate) fn known_terminal(id: &str) -> Option<&'static WindowsTerminal> {
    KNOWN_TERMINALS.iter().find(|t| t.known.id == id)
}

/// How a stored setting reads on Windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stored {
    /// Empty, or the registry default: whatever the default order finds first.
    Default,
    /// A known terminal or a custom pick.
    Choice(WindowsChoice),
    /// Something this build can't launch (another OS's bundle id, an id that left the
    /// table).
    Unknown,
}

/// Reads a stored setting. A custom pick is an absolute Windows path (`C:\…`, or a
/// `\\server\share\…` UNC path), told apart structurally from an id.
pub(crate) fn parse_choice(setting: &str) -> Stored {
    let setting = setting.trim();
    if setting.is_empty() || setting == REGISTRY_DEFAULT {
        return Stored::Default;
    }
    if is_absolute_windows_path(setting) {
        return Stored::Choice(TerminalChoice::CustomApp(PathBuf::from(setting)));
    }
    match known_terminal(setting) {
        Some(terminal) => Stored::Choice(TerminalChoice::Known(&terminal.known)),
        None => Stored::Unknown,
    }
}

/// `C:\…` / `C:/…` or `\\…`, read as text so this works on every host.
fn is_absolute_windows_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/');
    drive || text.starts_with(r"\\")
}

/// The first installed terminal in the default order, or Command Prompt, which every
/// Windows carries, when the probe finds none at all.
pub(crate) fn default_choice(is_installed: impl Fn(&WindowsChoice) -> bool) -> WindowsChoice {
    KNOWN_TERMINALS
        .iter()
        .map(|t| TerminalChoice::Known(&t.known))
        .find(|choice| is_installed(choice))
        .unwrap_or_else(|| {
            let cmd = known_terminal(CMD_ID).expect("the Windows table always carries Command Prompt");
            TerminalChoice::Known(&cmd.known)
        })
}

/// Resolves a stored setting to the app to launch. The default is never "missing":
/// it's whatever the order finds. A choice that's gone, or one this build can't read,
/// falls back to the default and says so.
pub(crate) fn resolve(
    setting: &str,
    is_installed: impl Fn(&WindowsChoice) -> bool,
) -> (WindowsChoice, OpenTerminalOutcome) {
    let parsed = match parse_choice(setting) {
        Stored::Default => return (default_choice(&is_installed), OpenTerminalOutcome::Opened),
        Stored::Choice(choice) => Some(choice),
        Stored::Unknown => None,
    };
    super::super::resolve_choice(parsed, &is_installed, || default_choice(&is_installed))
}

/// What to call a stored choice once its app is gone, with no I/O. `None` for the
/// default (it's never the missing one) and for an id this build doesn't know.
pub(crate) fn choice_display_name(setting: &str) -> Option<String> {
    match parse_choice(setting) {
        Stored::Choice(choice) => choice.name_without_io(),
        Stored::Default | Stored::Unknown => None,
    }
}

/// Everything `CreateProcessW` needs for one launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaunchPlan {
    /// The executable, absolute.
    pub(crate) program: PathBuf,
    /// Its arguments: constants only, ❌ never the folder.
    pub(crate) args: &'static [&'static str],
    /// The folder, as the process's working directory (`lpCurrentDirectory`).
    pub(crate) cwd: PathBuf,
    /// Start it on a console of its own (`CREATE_NEW_CONSOLE`).
    pub(crate) new_console: bool,
}

/// The launch for `recipe` with `program` (the executable the probe found, or the
/// custom pick) in `dir`. A custom pick launches like a console shell: the folder as
/// its working directory, no arguments, and a console of its own, which a GUI program
/// ignores.
pub(crate) fn launch_plan(recipe: Option<WindowsRecipe>, program: PathBuf, dir: &Path) -> LaunchPlan {
    let (args, new_console): (&'static [&'static str], bool) = match recipe {
        Some(WindowsRecipe::WindowsTerminal) => (&["-d", "."], false),
        Some(WindowsRecipe::ConsoleShell) | None => (&[], true),
    };
    LaunchPlan {
        program,
        args,
        cwd: dir.to_path_buf(),
        new_console,
    }
}

/// The plan's command line, as `CreateProcessW` takes it: the program quoted, then
/// the constant arguments. `None` for a program path holding a `"`, which no Windows
/// file name can, so it can't be quoted faithfully and isn't launched.
#[cfg_attr(
    feature = "playwright-e2e",
    allow(dead_code, reason = "the E2E build records the folder instead of launching")
)]
pub(crate) fn command_line(plan: &LaunchPlan) -> Option<String> {
    let program = plan.program.to_str()?;
    if program.contains('"') {
        return None;
    }
    let mut line = format!("\"{program}\"");
    for arg in plan.args {
        line.push(' ');
        line.push_str(arg);
    }
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(choice: &WindowsChoice) -> String {
        choice.id()
    }

    #[test]
    fn ids_are_unique_and_none_looks_like_a_path() {
        let mut ids: Vec<&str> = KNOWN_TERMINALS.iter().map(|t| t.known.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two entries share an id");
        assert!(ids.iter().all(|id| !is_absolute_windows_path(id)));
    }

    #[test]
    fn the_default_order_is_wt_pwsh_powershell_cmd() {
        let order: Vec<&str> = KNOWN_TERMINALS.iter().map(|t| t.known.id).collect();
        assert_eq!(order, vec![WINDOWS_TERMINAL_ID, PWSH_ID, WINDOWS_POWERSHELL_ID, CMD_ID]);
    }

    #[test]
    fn empty_and_the_macos_registry_default_read_as_the_default() {
        assert_eq!(parse_choice(""), Stored::Default);
        assert_eq!(parse_choice("  "), Stored::Default);
        assert_eq!(parse_choice("com.apple.Terminal"), Stored::Default);
    }

    #[test]
    fn a_known_id_and_an_absolute_path_are_choices() {
        assert_eq!(
            parse_choice("pwsh"),
            Stored::Choice(TerminalChoice::Known(&known_terminal(PWSH_ID).expect("pwsh").known))
        );
        assert_eq!(
            parse_choice(r"C:\Tools\Alacritty\alacritty.exe"),
            Stored::Choice(TerminalChoice::CustomApp(PathBuf::from(
                r"C:\Tools\Alacritty\alacritty.exe"
            )))
        );
        assert!(matches!(
            parse_choice(r"\\testnas.local\tools\term.exe"),
            Stored::Choice(TerminalChoice::CustomApp(_))
        ));
    }

    #[test]
    fn another_oss_bundle_id_is_unknown() {
        assert_eq!(parse_choice("com.googlecode.iterm2"), Stored::Unknown);
    }

    #[test]
    fn the_default_is_the_first_installed_and_never_missing() {
        let only_powershell_and_cmd = |c: &WindowsChoice| matches!(id(c).as_str(), WINDOWS_POWERSHELL_ID | CMD_ID);
        let (choice, outcome) = resolve("com.apple.Terminal", only_powershell_and_cmd);
        assert_eq!(id(&choice), WINDOWS_POWERSHELL_ID);
        assert_eq!(outcome, OpenTerminalOutcome::Opened);

        let (choice, outcome) = resolve("", |_| true);
        assert_eq!(id(&choice), WINDOWS_TERMINAL_ID);
        assert_eq!(outcome, OpenTerminalOutcome::Opened);
    }

    #[test]
    fn with_nothing_found_the_default_is_command_prompt() {
        let (choice, outcome) = resolve("", |_| false);
        assert_eq!(id(&choice), CMD_ID);
        assert_eq!(outcome, OpenTerminalOutcome::Opened);
    }

    #[test]
    fn an_installed_choice_is_used_as_is() {
        let (choice, outcome) = resolve("cmd", |_| true);
        assert_eq!(id(&choice), CMD_ID);
        assert_eq!(outcome, OpenTerminalOutcome::Opened);
    }

    #[test]
    fn a_gone_choice_falls_back_to_the_default_and_says_so() {
        let not_pwsh = |c: &WindowsChoice| id(c) != PWSH_ID;
        let (choice, outcome) = resolve("pwsh", not_pwsh);
        assert_eq!(id(&choice), WINDOWS_TERMINAL_ID);
        assert_eq!(outcome, OpenTerminalOutcome::AppMissingOpenedTerminalInstead);

        let (choice, outcome) = resolve(r"C:\Tools\gone.exe", |c: &WindowsChoice| {
            matches!(c, TerminalChoice::Known(_))
        });
        assert_eq!(id(&choice), WINDOWS_TERMINAL_ID);
        assert_eq!(outcome, OpenTerminalOutcome::AppMissingOpenedTerminalInstead);
    }

    #[test]
    fn an_unknown_id_falls_back_and_says_so() {
        let (_, outcome) = resolve("com.googlecode.iterm2", |_| true);
        assert_eq!(outcome, OpenTerminalOutcome::AppMissingOpenedTerminalInstead);
    }

    #[test]
    fn names_for_the_gone_toast() {
        assert_eq!(choice_display_name("pwsh"), Some("PowerShell".to_string()));
        assert_eq!(
            choice_display_name(r"C:\Tools\Alacritty\alacritty.exe"),
            // `Path` splits on `\` only on Windows, so a Mac reads the whole text as one
            // name; the stem is what matters.
            Some(
                PathBuf::from(r"C:\Tools\Alacritty\alacritty.exe")
                    .file_stem()
                    .expect("a stem")
                    .to_string_lossy()
                    .into_owned()
            )
        );
        assert_eq!(choice_display_name("com.apple.Terminal"), None);
        assert_eq!(choice_display_name("com.googlecode.iterm2"), None);
    }

    #[test]
    fn windows_terminal_starts_in_the_folder_with_dot_and_no_folder_in_its_argv() {
        let dir = Path::new(r"C:\Users\test_user_10\a;b & c");
        let plan = launch_plan(
            Some(WindowsRecipe::WindowsTerminal),
            PathBuf::from(r"C:\Users\test_user_10\AppData\Local\Microsoft\WindowsApps\wt.exe"),
            dir,
        );
        assert_eq!(plan.args, &["-d", "."]);
        assert_eq!(plan.cwd, dir);
        assert!(!plan.new_console);
        let line = command_line(&plan).expect("quotable");
        assert_eq!(
            line,
            r#""C:\Users\test_user_10\AppData\Local\Microsoft\WindowsApps\wt.exe" -d ."#
        );
        assert!(
            !line.contains("a;b"),
            "the folder must never reach wt's `;`-split command line"
        );
    }

    #[test]
    fn a_console_shell_starts_in_the_folder_on_its_own_console() {
        let dir = Path::new(r"C:\Users\test_user_10\code");
        let plan = launch_plan(
            Some(WindowsRecipe::ConsoleShell),
            PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            dir,
        );
        assert_eq!(plan.args, &[] as &[&str]);
        assert_eq!(plan.cwd, dir);
        assert!(plan.new_console);
        assert_eq!(command_line(&plan).as_deref(), Some(r#""C:\Windows\System32\cmd.exe""#));
    }

    #[test]
    fn a_custom_pick_launches_like_a_console_shell() {
        let plan = launch_plan(None, PathBuf::from(r"C:\Tools\term.exe"), Path::new(r"D:\work"));
        assert!(plan.new_console);
        assert!(plan.args.is_empty());
        assert_eq!(plan.cwd, Path::new(r"D:\work"));
    }

    #[test]
    fn a_program_path_with_a_quote_is_not_launched() {
        let plan = launch_plan(None, PathBuf::from(r#"C:\x"y.exe"#), Path::new(r"C:\"));
        assert_eq!(command_line(&plan), None);
    }
}
