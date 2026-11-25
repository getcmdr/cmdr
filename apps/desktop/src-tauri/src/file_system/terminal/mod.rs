//! "Open terminal here": which terminal apps Cmdr knows, how each one takes a
//! folder, and what actually happens when the action fires.
//!
//! Neither macOS nor Windows hands an app a "default terminal" it can launch at a
//! folder, so Cmdr carries its own table per OS, the way every other app offering
//! this does. This module is the part both share:
//!
//! - The wire types the IPC commands in `commands/file_actions.rs` answer with
//!   (`OpenTerminalOutcome`, `OpenTerminalError`, `TerminalAppList`).
//! - The table vocabulary (`KnownTerminal`, `TerminalChoice`) and the fallback
//!   rule (`resolve_choice`), generic over each OS's launch recipe.
//! - The path-less refusal, which keys on the pane's volume.
//!
//! Each OS's table, its installed-ness probe, and its launch live behind the
//! `platform/` seam: macOS asks `NSWorkspace` per bundle id and launches through
//! `open`; Windows looks for four known executables and starts them with
//! `CreateProcessW`, the folder as their working directory. The Windows table and
//! its pure launch plan (`platform/windows_table.rs`) compile on every OS's test build, so
//! their unit tests run on a Mac too.
//!
//! macOS and Windows only: Linux has no default terminal either
//! (`x-terminal-emulator` on Debian, the emerging `xdg-terminal-exec`), and no arm
//! yet. The module, the `commands/file_actions.rs` wrappers, and the `ipc.rs`
//! registration are all gated on the two, and the `open_terminal_here` capability
//! hides every surface on Linux.
//!
//! Rationale, and why there's no window-vs-tab control: `DETAILS.md` § "Open
//! terminal here".

mod platform;

use std::path::PathBuf;

use cmdr_fs::app_path::AppPath;

/// One terminal app Cmdr knows how to launch a folder in, with `R`, the OS's own
/// description of how that app takes the folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownTerminal<R: 'static> {
    /// The value stored in the setting: a bundle id on macOS, a short stable name on
    /// Windows.
    pub id: &'static str,
    /// The app's own name, shown in the settings dropdown. Product names, so they
    /// keep their vendor casing (`iTerm2`, `kitty`, `WezTerm`).
    pub display_name: &'static str,
    pub recipe: R,
}

/// Which app the "open terminal here" setting names.
///
/// The stored setting is one string holding either kind, so the two are told apart
/// structurally: a custom pick is an absolute path, and a table id never is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalChoice<R: 'static> {
    /// One of the OS's known terminals, with its recipe.
    Known(&'static KnownTerminal<R>),
    /// An app the user picked by hand through "Choose an app…", held as its absolute
    /// path: a `.app` bundle on macOS, an `.exe` on Windows.
    CustomApp(PathBuf),
}

impl<R> TerminalChoice<R> {
    /// The value that goes back into the setting, and the id the frontend uses to
    /// tell one dropdown option from another.
    pub fn id(&self) -> String {
        match self {
            Self::Known(t) => t.id.to_string(),
            Self::CustomApp(path) => path.to_string_lossy().into_owned(),
        }
    }

    /// The name to call this choice by, with no I/O: a known terminal answers from
    /// the table, a custom pick with its file stem, the name the user picked it by.
    /// Both still answer once the app is gone, which is when the toast asks.
    pub fn name_without_io(&self) -> Option<String> {
        match self {
            Self::Known(terminal) => Some(terminal.display_name.to_string()),
            Self::CustomApp(path) => path.file_stem().map(|stem| stem.to_string_lossy().into_owned()),
        }
    }
}

/// Resolves a parsed setting to the app that will actually be launched.
///
/// Pure given the "is this installed?" answer, so the fallback rule is testable
/// without probing anything: an installed choice is used as is, anything else
/// (`None` is a setting this build can't read) falls back and reports that it did.
pub(crate) fn resolve_choice<C>(
    parsed: Option<C>,
    is_installed: impl Fn(&C) -> bool,
    fallback: impl FnOnce() -> C,
) -> (C, OpenTerminalOutcome) {
    match parsed {
        Some(choice) if is_installed(&choice) => (choice, OpenTerminalOutcome::Opened),
        // Either the setting named something this build can't launch, or the app
        // it named is gone. Both land on the fallback, and both owe the user a word.
        _ => (fallback(), OpenTerminalOutcome::AppMissingOpenedTerminalInstead),
    }
}

/// What `open_terminal_here` did, so the frontend acts on a variant rather than
/// reading a sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OpenTerminalOutcome {
    /// The chosen terminal was launched at the folder.
    Opened,
    /// The chosen app isn't installed anymore, so the OS's fallback terminal opened
    /// instead (Terminal on macOS, the first installed of the table's default order
    /// on Windows). The frontend says so and resets the setting.
    AppMissingOpenedTerminalInstead,
    /// The pane isn't on a path a shell can `cd` into (MTP, ADB, a share whose
    /// mount went away), so nothing was launched.
    NotALocalPath,
    /// Windows: the folder has no plain spelling a process's working directory takes
    /// as this same folder (past 259 characters, a name ending in a dot or space, a
    /// device name like `CON`), so nothing was launched.
    NoPlainPath,
    /// Windows: Command Prompt can't start in a network folder (`\\server\share`): it says
    /// "UNC paths are not supported" and starts in the Windows folder instead. So nothing was
    /// launched; PowerShell and Windows Terminal take the folder.
    CommandPromptCantOpenNetworkFolder,
}

/// Why `open_terminal_here` couldn't answer at all. Distinct from
/// [`OpenTerminalOutcome`], which reports things that DID happen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum OpenTerminalError {
    /// The launch couldn't be started (`open` on macOS, `CreateProcessW` on
    /// Windows). Carries the OS error code where there is one, so nothing has to read
    /// the message.
    LaunchRefused { errno: Option<i32> },
    /// The launch didn't finish inside the command's deadline.
    TimedOut,
}

impl std::fmt::Display for OpenTerminalError {
    /// ❗ For logs only; the frontend words the variant.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LaunchRefused { errno } => write!(f, "launch refused (errno {errno:?})"),
            Self::TimedOut => f.write_str("timed out"),
        }
    }
}

impl std::error::Error for OpenTerminalError {}

/// One installed terminal, as the settings dropdown needs it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalApp {
    /// Exactly what goes into the setting: a table id for a known terminal, an
    /// absolute path for a custom pick.
    pub id: String,
    pub display_name: String,
    /// The app's icon as a data URL. Absent when the app carries no readable icon.
    pub icon: Option<String>,
    /// Whether the app is running right now. The first-use picker prefers a
    /// running terminal when exactly one is. Always `false` on Windows, where the
    /// signal means nothing (`platform/windows.rs` says why).
    pub is_running: bool,
}

/// The terminal apps installed on this machine, plus which one is chosen.
// DEFAULT-OK: an empty list with nothing chosen is what the command's deadline has
// to answer with, and it's honest: "the query didn't finish" carries no claim about
// which terminals exist. The `TimedOut` flag beside it says which case it was.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAppList {
    /// Installed apps in table order, plus the custom pick last when there is one.
    pub apps: Vec<TerminalApp>,
    /// The id of the currently chosen app, present in `apps`. Absent when the
    /// chosen app has been uninstalled, which is the frontend's cue to reset.
    pub chosen_id: Option<String>,
    /// The id of the app the setting's default (and every fallback) launches on this
    /// machine, present in `apps`: Terminal on macOS, the first installed of Windows
    /// Terminal, PowerShell 7, Windows PowerShell, and Command Prompt on Windows. The
    /// first-use picker and the row's fallback display read it, so neither names an
    /// OS's terminal itself.
    pub default_id: Option<String>,
}

/// The terminal apps installed on this machine, in dropdown order, plus which one
/// the stored `setting` names.
///
/// Asked fresh every time the settings row renders: installed-ness is one lookup
/// per app, so there's nothing to cache and no "Refresh" button to offer.
pub fn list_terminal_apps(setting: &str) -> TerminalAppList {
    platform::list_terminal_apps(setting)
}

/// The name to call a stored choice when the app it names has just gone missing.
///
/// I/O-free on purpose: the app is gone, so there's nothing left to read a name out
/// of. `None` means Cmdr has no name to offer (an id that left the table), and the
/// caller words the nameless variant rather than putting an id in front of anyone.
pub fn choice_display_name(setting: &str) -> Option<String> {
    platform::choice_display_name(setting)
}

/// Opens `dir` in the terminal the stored `setting` names.
///
/// Reports what happened rather than whether it worked: a pane on a volume with no
/// OS-visible paths launches nothing, and a setting naming an app that's been
/// uninstalled falls back to the OS's default terminal and says so.
pub fn open_terminal_here(
    volume_id: &str,
    dir: &AppPath,
    setting: &str,
) -> Result<OpenTerminalOutcome, OpenTerminalError> {
    if !volume_paths_are_os_visible(volume_id) {
        return Ok(OpenTerminalOutcome::NotALocalPath);
    }
    platform::open_in_terminal(dir, setting)
}

/// Whether the pane's volume hands out paths the OS (and so a shell) can
/// reach: local disks, OS-mounted shares, direct SMB while its share is
/// mounted. False for MTP, ADB, and a share whose mount went away.
///
/// ❌ Not a test on the path string, and ❌ not `supports_local_fs_access()`:
/// that asks whether Cmdr reads through `std::fs`, and direct SMB answers
/// `false` while its `/Volumes/…` paths are exactly what a shell can `cd`
/// into. Same reading Quick Look and the drag commands take.
fn volume_paths_are_os_visible(volume_id: &str) -> bool {
    match crate::file_system::volume::manager::get_volume_manager().get(volume_id) {
        Some(volume) => volume.paths_are_os_visible(),
        None => {
            // An id the registry doesn't know is an unmount race, not a
            // verdict. The frontend gates on the pane's volume kind before
            // it ever gets here, so assume yes rather than silently refusing.
            log::debug!(target: "terminal", "volume {volume_id} not found; assuming its paths are OS-visible");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installed_choice_is_used_as_is() {
        let (choice, outcome) = resolve_choice(Some("warp"), |_| true, || "terminal");
        assert_eq!(choice, "warp");
        assert_eq!(outcome, OpenTerminalOutcome::Opened);
    }

    #[test]
    fn an_uninstalled_choice_falls_back_and_says_so() {
        let (choice, outcome) = resolve_choice(Some("warp"), |_| false, || "terminal");
        assert_eq!(choice, "terminal");
        assert_eq!(outcome, OpenTerminalOutcome::AppMissingOpenedTerminalInstead);
    }

    #[test]
    fn a_setting_this_build_cannot_read_falls_back_and_says_so() {
        let (choice, outcome) = resolve_choice(None, |_: &&str| true, || "terminal");
        assert_eq!(choice, "terminal");
        assert_eq!(outcome, OpenTerminalOutcome::AppMissingOpenedTerminalInstead);
    }

    #[test]
    fn a_custom_pick_is_named_by_its_file_stem() {
        let choice: TerminalChoice<()> = TerminalChoice::CustomApp(PathBuf::from("/Applications/Terminus.app"));
        assert_eq!(choice.name_without_io(), Some("Terminus".to_string()));
    }
}
