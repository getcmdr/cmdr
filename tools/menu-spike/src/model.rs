//! Cmdr's real Windows menu bar and file context menu, read from `cmdr-menu`, flattened into a
//! plain tree the three modes share. English labels come from the app's own catalog.

use std::collections::HashMap;
use std::sync::OnceLock;

use cmdr_menu::bar::menus_on;
use cmdr_menu::context::file::{FileMenuFacts, PaneFacts, TargetFacts, file_menu};
use cmdr_menu::context::{ContextLabel, ContextRow, Shortcut};
use cmdr_menu::spec::{EntryKind, Label, Platform, SubmenuSpec};
use serde::Serialize;

const PLATFORM: Platform = Platform::Windows;

/// One menu row. `mnemonic` is the index (in chars) of the letter Alt+letter picks, if any.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Row {
    Item {
        id: String,
        label: String,
        mnemonic: Option<usize>,
        /// A Tauri accelerator the native bar REGISTERS (it can fire on its own).
        accelerator: Option<String>,
        /// A shortcut the menu only SHOWS (the page owns the key).
        shown: Option<String>,
        enabled: bool,
        checked: Option<bool>,
    },
    Submenu {
        label: String,
        mnemonic: Option<usize>,
        rows: Vec<Row>,
    },
    Separator,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopMenu {
    pub label: String,
    pub mnemonic: Option<usize>,
    pub rows: Vec<Row>,
}

/// The bar, top menus in order.
pub fn bar() -> Vec<TopMenu> {
    let mut titles = Mnemonics::default();
    menus_on(PLATFORM)
        .map(|bar_menu| {
            let label = text(bar_menu.submenu.title);
            TopMenu {
                mnemonic: titles.pick(&label),
                label,
                rows: rows(&bar_menu.submenu),
            }
        })
        .collect()
}

fn rows(spec: &'static SubmenuSpec) -> Vec<Row> {
    let mut mnemonics = Mnemonics::default();
    spec.entries_on(PLATFORM)
        .map(|entry| match entry {
            EntryKind::Item(item) => {
                let label = text(item.label);
                Row::Item {
                    id: item.id.to_string(),
                    mnemonic: mnemonics.pick(&label),
                    label,
                    accelerator: item.accelerator.on(PLATFORM).map(str::to_string),
                    shown: item.display_accelerator.on(PLATFORM).map(str::to_string),
                    enabled: item.enabled,
                    checked: None,
                }
            }
            EntryKind::Check(check) => {
                let label = text(check.label);
                Row::Item {
                    id: check.id.to_string(),
                    mnemonic: mnemonics.pick(&label),
                    label,
                    accelerator: check.accelerator.on(PLATFORM).map(str::to_string),
                    shown: None,
                    enabled: true,
                    checked: Some(false),
                }
            }
            EntryKind::Submenu(nested) => {
                let label = text(nested.title);
                Row::Submenu {
                    mnemonic: mnemonics.pick(&label),
                    label,
                    rows: rows(nested),
                }
            }
            EntryKind::Predefined(kind, label) => {
                let label = text(*label);
                Row::Item {
                    id: format!("predefined:{kind:?}"),
                    mnemonic: mnemonics.pick(&label),
                    label,
                    accelerator: None,
                    shown: None,
                    enabled: true,
                    checked: None,
                }
            }
            EntryKind::Separator => Row::Separator,
        })
        .collect()
}

/// The real file context menu for one right-clicked file, as the app builds it on Windows.
pub fn file_context_menu() -> Vec<Row> {
    let facts = FileMenuFacts {
        filename: "report.pdf",
        is_directory: false,
        pane: PaneFacts {
            can_open_terminal_here: true,
            can_favorite: true,
            ..PaneFacts::default()
        },
        image_index: Default::default(),
        target: TargetFacts {
            count: 1,
            count_text: None,
            size_text: Some("1.2 MB"),
        },
        same_kind: None,
    };
    context_rows(file_menu(&facts, PLATFORM))
}

fn context_rows(rows: Vec<ContextRow>) -> Vec<Row> {
    let mut mnemonics = Mnemonics::default();
    rows.into_iter()
        .filter_map(|row| match row {
            ContextRow::Item(item) | ContextRow::Check { item, .. } => {
                let label = context_text(&item.label);
                let shown = match item.shortcut {
                    Shortcut::OfMenuId(id) => cmdr_menu::bar::spec_accelerator(id, PLATFORM)
                        .or_else(|| cmdr_menu::bar::spec_display_accelerator(id, PLATFORM))
                        .map(str::to_string),
                    Shortcut::Fixed(key) => Some(key.to_string()),
                    Shortcut::None | Shortcut::OfCommand(_) => None,
                };
                Some(Row::Item {
                    id: item.id,
                    mnemonic: mnemonics.pick(&label),
                    label,
                    accelerator: None,
                    shown,
                    enabled: item.enabled,
                    checked: None,
                })
            }
            ContextRow::Submenu { label, rows, .. } => {
                let label = context_text(&label);
                Some(Row::Submenu {
                    mnemonic: mnemonics.pick(&label),
                    label,
                    rows: context_rows(rows),
                })
            }
            ContextRow::Header(text) => Some(Row::Item {
                id: "context:header".into(),
                label: text,
                mnemonic: None,
                accelerator: None,
                shown: None,
                enabled: false,
                checked: None,
            }),
            ContextRow::Separator => Some(Row::Separator),
            ContextRow::Slot(_) => None,
        })
        .collect()
}

fn context_text(label: &ContextLabel) -> String {
    match label {
        ContextLabel::Text(text) => text.clone(),
        ContextLabel::Key(key) => {
            let mut text = catalog(key.key);
            for (token, value) in &key.args {
                text = text.replace(&format!("{{{token}}}"), value);
            }
            text
        }
    }
}

fn text(label: Label) -> String {
    match label.catalog_key(false) {
        Some(key) => catalog(key),
        None => "Cmdr".to_string(),
    }
}

/// `key` in the English catalog, with its `{platform, select, …}` resolved for Windows and any
/// remaining `{token}` filled with a sample value.
fn catalog(key: &str) -> String {
    static CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        let raw = include_str!("../../../apps/desktop/src/lib/intl/messages/en/menu.json");
        let parsed: HashMap<String, serde_json::Value> = serde_json::from_str(raw).unwrap_or_default();
        parsed
            .into_iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
            .collect()
    });
    let Some(value) = catalog.get(key) else {
        return key.to_string();
    };
    let value = resolve_select(value);
    value
        .replace("{name}", "report.pdf")
        .replace("{extension}", "pdf")
        .replace("{countText}", "3")
}

/// Picks the `windows` (else `other`) branch of a top-level `{platform, select, …}`.
fn resolve_select(value: &str) -> String {
    let Some(rest) = value.strip_prefix("{platform, select,") else {
        return value.to_string();
    };
    let mut branches: Vec<(String, String)> = Vec::new();
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        while i < chars.len() && chars[i] != '{' && chars[i] != '}' {
            i += 1;
        }
        if i >= chars.len() || chars[i] == '}' {
            break;
        }
        let name: String = chars[start..i].iter().collect::<String>().trim().to_string();
        let mut depth = 0;
        let body_start = i + 1;
        while i < chars.len() {
            match chars[i] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        branches.push((name, chars[body_start..i.min(chars.len())].iter().collect()));
        i += 1;
    }
    branches
        .iter()
        .find(|(name, _)| name == "windows")
        .or_else(|| branches.iter().find(|(name, _)| name == "other"))
        .map_or_else(|| value.to_string(), |(_, body)| body.clone())
}

/// The desktop's mnemonic rule (`apps/desktop/src-tauri/src/menu/mnemonics.rs`): the first free
/// word-initial letter, then any free letter.
#[derive(Default)]
struct Mnemonics {
    taken: Vec<char>,
}

impl Mnemonics {
    fn pick(&mut self, label: &str) -> Option<usize> {
        let chars: Vec<char> = label.chars().collect();
        let usable: Vec<usize> = (0..chars.len()).filter(|&i| chars[i].is_alphanumeric()).collect();
        let initial: Vec<usize> = usable
            .iter()
            .copied()
            .filter(|&i| i == 0 || !chars[i - 1].is_alphanumeric())
            .collect();
        let rest = usable.into_iter().filter(|i| !initial.contains(i));
        for index in initial.clone().into_iter().chain(rest) {
            let letter = chars[index].to_lowercase().next().unwrap_or(chars[index]);
            if !self.taken.contains(&letter) {
                self.taken.push(letter);
                return Some(index);
            }
        }
        None
    }
}

/// `label` with Win32's `&` before its mnemonic, and any literal `&` doubled.
pub fn with_ampersand(label: &str, mnemonic: Option<usize>) -> String {
    let mut out = String::new();
    for (i, c) in label.chars().enumerate() {
        if Some(i) == mnemonic {
            out.push('&');
        }
        if c == '&' {
            out.push('&');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_picks_the_windows_branch() {
        let select = "{platform, select, macos {Show in Finder} windows {Show in File Explorer} other {x}}"; // allowed-pluralize-noun: an ICU select, not a count
        assert_eq!(resolve_select(select), "Show in File Explorer");
        assert_eq!(
            resolve_select("{platform, select, windows {Exit Cmdr} other {Quit Cmdr}}"),
            "Exit Cmdr"
        );
        assert_eq!(resolve_select("Plain"), "Plain");
    }

    #[test]
    fn the_bar_has_titles_and_rows() {
        let bar = bar();
        assert!(bar.len() >= 5, "{bar:?}");
        assert_eq!(bar[0].label, "File");
        assert!(bar.iter().all(|m| !m.rows.is_empty()));
        assert!(!file_context_menu().is_empty());
    }
}
