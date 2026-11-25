//! Which app F4 opens a file in: the text editors macOS lists, the one the user
//! chose, and what happened when the file was handed over.
//!
//! The list is LaunchServices' own answer to "which apps EDIT plain text", so it
//! carries whatever macOS carries, oddities included. ❌ No editor table, no
//! `/Applications` scan. The stored choice arrives as an argument, because the
//! frontend owns the settings store.
//!
//! The wire types compile on every platform: Linux's `xdg-open` arm and the
//! `playwright-e2e` arms of `open_in_editor` (`commands/file_actions/platform/`) answer with them
//! too. Each OS's list and launch is in `platform/`: LaunchServices on macOS, the Shell's
//! plain-text handlers on Windows, nothing on Linux.
//!
//! Evidence, the pick canonicalization, and what's verified about `open -t`:
//! `DETAILS.md` § "Text editor".

/// What `open_in_editor` did with the file, so the frontend acts on a variant rather
/// than reading a sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EditorOpenOutcome {
    /// The chosen app got the file (the system default, when that's the choice).
    /// ❗ It means the OS took the request (`open` on macOS, the Shell or the process start on
    /// Windows), not that a window appeared.
    Opened,
    /// The chosen app isn't on this computer right now, so the system default got the file.
    /// The frontend resets the setting and says so.
    ChosenAppMissingOpenedDefaultInstead,
}

/// The answer to one F4.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorOpenReport {
    pub outcome: EditorOpenOutcome,
    /// The app that got the file, named the way Finder shows it. Filled in only when
    /// something words it: a fallback outcome (the missing-app toast), or a press whose
    /// caller asked about other editors (the one-time hint). `None` otherwise, and when
    /// there's nothing to read a name from (no system default resolves, or off macOS).
    pub opened_in_name: Option<String>,
    /// `None` unless the caller asked. When asked: whether macOS lists any installed
    /// text editor besides the system default, which is what the one-time hint needs.
    pub other_editors_installed: Option<bool>,
}

/// Why `open_in_editor` couldn't answer at all. Distinct from [`EditorOpenOutcome`],
/// which reports things that DID happen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum OpenInEditorError {
    /// The launcher couldn't be spawned. Carries the OS errno where there is one, so
    /// nothing has to read a message.
    LaunchRefused { errno: Option<i32> },
    /// The launch didn't finish inside the command's deadline.
    TimedOut,
    /// Windows: the file's path has no plain spelling an editor reads as this same file (past
    /// 259 characters, a name ending in a dot or space, a device name like `CON`). Nothing
    /// launched.
    NoPlainPath,
}

impl std::fmt::Display for OpenInEditorError {
    /// ❗ For logs only; the frontend words the variant.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LaunchRefused { errno } => write!(f, "launch refused (errno {errno:?})"),
            Self::TimedOut => f.write_str("timed out"),
            Self::NoPlainPath => f.write_str("no plain path"),
        }
    }
}

impl std::error::Error for OpenInEditorError {}

/// The stored `behavior.textEditorApp` value meaning "whatever macOS opens plain text in". The
/// frontend mirrors it as `SYSTEM_DEFAULT_EDITOR_CHOICE`. Cross-platform so `set_setting`'s question
/// can word it on any OS.
pub(crate) const SYSTEM_DEFAULT_CHOICE: &str = "system";

/// One text editor, as the settings row needs it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TextEditorApp {
    /// Exactly what goes into the setting: on macOS a bundle id or an absolute `.app` path; on
    /// Windows a Shell handler's id (`IAssocHandler::GetName`) or an absolute program path.
    pub id: String,
    /// The app's name the way the OS's file manager shows it (Finder, File Explorer).
    pub display_name: String,
    /// The app's icon as a base64 WebP data URL (read from its bundle on macOS, the Shell's
    /// on Windows). Absent when there's no readable icon.
    pub icon: Option<String>,
}

/// The text editors on this Mac, plus the system default and which one is chosen.
// DEFAULT-OK: an empty list with no default and nothing chosen is what the command's
// deadline has to answer with, and it claims nothing about which editors exist. The
// `TimedOut` flag beside it says which case it was.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TextEditorList {
    /// What the system default is called, for the "System default (…)" row. `None`
    /// when nothing on this Mac claims plain text.
    pub default_app_name: Option<String>,
    pub default_app_icon: Option<String>,
    /// Every other listed editor, plus the chosen app when macOS doesn't list it.
    /// ❗ In no particular order: LaunchServices' own order shifts between calls,
    /// so the frontend sorts by name.
    pub apps: Vec<TextEditorApp>,
    /// The stored choice in canonical form: `system`, or the `id` of one of `apps`.
    /// `None` once the chosen app is gone, which the row displays as the default
    /// without writing anything.
    pub chosen_id: Option<String>,
}

pub(crate) mod platform;
