//! Each OS's terminal table, its installed-ness probe, and its launch. A platform seam
//! (`docs/guides/platform-seams.md`), with no Linux arm: the whole `terminal` module is
//! gated on macOS and Windows (`../mod.rs` says why).

use cmdr_fs::app_path::AppPath;

use super::{OpenTerminalError, OpenTerminalOutcome, TerminalAppList};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

/// Windows' pure half: the table, the setting's reading, and the launch plan. Compiled
/// into every OS's test build too, so its tests run on a Mac; only the Windows arm
/// reads all of it.
#[cfg(any(target_os = "windows", test))]
#[cfg_attr(
    test,
    allow(dead_code, reason = "other OSes compile the Windows table only for its pure tests")
)]
mod windows_table;

const _: fn(&str) -> TerminalAppList = list_terminal_apps;
const _: fn(&str) -> Option<String> = choice_display_name;
const _: fn(&AppPath, &str) -> Result<OpenTerminalOutcome, OpenTerminalError> = open_in_terminal;

/// The installed terminals in dropdown order, the custom pick last, plus which one
/// `setting` names and which one the default launches.
pub(super) use imp::list_terminal_apps;

/// What to call the app `setting` names, with no I/O; `None` when Cmdr has no name.
pub(super) use imp::choice_display_name;

/// Opens `dir` in the terminal `setting` names, falling back to the OS's default
/// terminal when that app is gone. The volume check already passed; this arm crosses
/// `dir` to the native spelling its launch takes, and answers `NotALocalPath` (or
/// `NoPlainPath` on Windows) when there's none.
pub(super) use imp::open_in_terminal;
