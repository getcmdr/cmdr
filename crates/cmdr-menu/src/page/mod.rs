//! The menus as a page draws them: the bar, the viewer bar, and every right-click menu, as rows
//! with their words, shown keys, `aria-keyshortcuts`, and state, for a host that draws its menus
//! in HTML instead of handing them to the OS.
//!
//! Two hosts draw from here: the browser demo (`cmdr-listing-wasm`'s `menu.rs` and
//! `context_menu.rs`), and the desktop wherever its menus live in the page (the in-window bar and
//! the in-page right-click menus off macOS, `apps/desktop/src-tauri/src/menu/page_bar.rs` and
//! `page_context.rs`). Both feed the same inputs through the same functions, so the page's menus
//! say what the native ones would. `DETAILS.md` § "The menus as a page draws them".
//!
//! - [`bar`]: [`BarInputs`] (what the menu-state pushes stored) into the bar's rows, plus the
//!   row-level diff a host sends on every change.
//! - [`viewer`]: the viewer window's bar.
//! - [`context`]: a right-click menu's [`crate::context::ContextRow`]s into rows.
//! - [`keys`]: a shortcut as the platform prints it and as `aria-keyshortcuts` spells it.

pub mod bar;
pub mod context;
pub mod keys;
pub mod viewer;

#[cfg(test)]
mod bar_keys_test;
#[cfg(test)]
mod bar_test;
#[cfg(test)]
mod keys_test;

use std::collections::HashMap;

use serde::Serialize;

use crate::bar::APP_MENU_TITLE;
use crate::labels::LabelText;
use crate::spec::Label;

pub use bar::{BarInputs, RowState, bar_view, changed_rows};
pub use keys::KeyHint;

/// One row of a menu as the page draws it.
///
/// The wire is the browser demo's (`lib/demo/menu-bar.svelte.ts` reads `kind`, `id`, `label`,
/// `shortcut`, `enabled`, `checked`, `rows`), so new facts are new fields beside those.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Row {
    /// A command item.
    #[serde(rename_all = "camelCase")]
    Item {
        /// The menu id a pick names.
        id: String,
        /// What it says.
        label: RowLabel,
        /// The shortcut drawn beside it, in the platform's own spelling (`⇧⌘P`, `Ctrl+Shift+P`).
        shortcut: Option<String>,
        /// The same shortcut in `aria-keyshortcuts` spelling (`Meta+Shift+P`, `Control+Shift+P`).
        aria_keyshortcuts: Option<String>,
        /// Whether a pick does something.
        enabled: bool,
        /// A picture drawn before the words, as an image URL (an app's icon in Windows' "Open
        /// with", a data URL). Absent for every row the crate builds: only the desktop's
        /// OS-filled rows carry one.
        #[specta(optional)]
        icon: Option<String>,
    },
    /// A check item: a checkbox, or one choice of a radio group.
    #[serde(rename_all = "camelCase")]
    Check {
        /// The menu id a pick names.
        id: String,
        /// What it says.
        label: RowLabel,
        /// The shortcut drawn beside it, in the platform's own spelling.
        shortcut: Option<String>,
        /// The same shortcut in `aria-keyshortcuts` spelling.
        aria_keyshortcuts: Option<String>,
        /// Whether a pick does something.
        enabled: bool,
        /// Whether it carries the checkmark.
        checked: bool,
        /// `menuitemcheckbox` or `menuitemradio`.
        check_kind: CheckKind,
    },
    /// A nested submenu.
    Submenu {
        /// Its id: the spec's where it has one, else its title's catalog key (unique per bar,
        /// `bar_test.rs` pins it), so a change event can name it.
        id: String,
        /// What it says.
        label: RowLabel,
        /// Whether it opens.
        enabled: bool,
        /// Its rows.
        rows: Vec<Row>,
    },
    /// A context menu's first line, naming what it acts on: a label, never picked.
    Header {
        /// What it says.
        label: RowLabel,
    },
    /// An item the OS wires (Hide, Quit, Services, Undo, Minimize: macOS's bar only). A page has
    /// no OS action behind it.
    System {
        /// What it says.
        label: RowLabel,
    },
    /// A separator.
    Separator,
}

/// Which of the two ARIA check roles a check row takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CheckKind {
    /// `menuitemcheckbox`: on or off by itself (Show hidden files, Word wrap).
    Checkbox,
    /// `menuitemradio`: one of a group, exactly one on (a pane's view mode, a Sort by column).
    Radio,
}

/// A row's words: a catalog key and its tokens, or a literal that is never translated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(untagged)]
pub enum RowLabel {
    /// A `menu.*` key in the frontend's catalog, with its `{token}` values.
    Key {
        /// The catalog key.
        key: String,
        /// Token name → value.
        args: HashMap<String, String>,
    },
    /// Words that stay as they are (the app menu's `Cmdr`, a context menu's header line).
    Text {
        /// The words.
        text: String,
    },
}

impl From<LabelText> for RowLabel {
    fn from(label: LabelText) -> Self {
        Self::Key {
            key: label.key.to_string(),
            args: label
                .args
                .into_iter()
                .map(|(token, value)| (token.to_string(), value))
                .collect(),
        }
    }
}

impl RowLabel {
    /// A catalog key with no tokens.
    pub fn key(key: &str) -> Self {
        Self::Key {
            key: key.to_string(),
            args: HashMap::new(),
        }
    }

    /// A spec label: its catalog key, or the app's name, which is never translated.
    pub fn of(spec: Label, has_existing_license: bool) -> Self {
        match spec.catalog_key(has_existing_license) {
            Some(catalog_key) => Self::key(catalog_key),
            None => Self::Text {
                text: APP_MENU_TITLE.to_string(),
            },
        }
    }
}

/// One top-level menu of a drawn bar.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct BarMenuView {
    /// Its menu id (`menu_file`, …), on every platform.
    pub id: String,
    /// What the bar says.
    pub title: RowLabel,
    /// Its rows.
    pub rows: Vec<Row>,
}

impl Row {
    /// The id a pick or a change event names this row by, for the rows that have one.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Item { id, .. } | Self::Check { id, .. } | Self::Submenu { id, .. } => Some(id),
            Self::Header { .. } | Self::System { .. } | Self::Separator => None,
        }
    }
}

/// `shortcut` + `aria_keyshortcuts` from one hint, the pair every row carries.
pub(crate) fn hint_fields(hint: Option<KeyHint>) -> (Option<String>, Option<String>) {
    match hint {
        Some(KeyHint { shown, aria }) => (Some(shown), aria),
        None => (None, None),
    }
}
