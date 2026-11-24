//! The main bar as a page draws it, from what the menu-state pushes stored.
//!
//! [`BarInputs`] is every fact a native bar keeps about itself: the inputs of the enabled fold,
//! the checks, the live labels, the rebinds. Each host stores the pushes its own way (the demo in
//! a `BarInputs`, the desktop across `MenuState`, read into one by `page_bar.rs`), and
//! [`bar_view`] turns it into rows with the same rules the native builder follows, so no host
//! decides a row. [`changed_rows`] is the row-level diff a host sends when the answer moves.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::accelerators::to_accelerator;
use crate::bar::{menus_on, rebound_row_key, spec_accelerator};
use crate::enabled::{MenuItemInputs, entry_enabled};
use crate::ids::{
    PIN_TAB_MENU_ID, SELECT_SAME_KIND_ID, UNDO_TRASH_ID, VIEW_MODE_BRIEF_LEFT_ID, VIEW_MODE_FULL_LEFT_ID,
    command_id_to_menu_id,
};
use crate::labels::{SameKindTarget, TrashUndoTarget, pin_tab_label, same_kind_label, undo_trash_label};
use crate::sort::RequestedSort;
use crate::spec::{CheckRole, EntryKind, Label, Pane, Platform, SubmenuSpec, ViewMode};

use super::{BarMenuView, CheckKind, KeyHint, Row, RowLabel, hint_fields};

/// Everything the main bar's rows are drawn from: what the menu-state commands pushed, as they
/// arrived. The desktop's `MenuState` holds the same facts (plus the muda handles).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarInputs {
    /// The main file explorer owns the menu (`activate_window_menu`).
    pub explorer_menu_active: bool,
    pub file_operations_blocked: bool,
    pub open_terminal_here_enabled: bool,
    pub reopen_closed_tab_enabled: bool,
    /// The commands the dialog gate refuses right now.
    pub refused: HashSet<String>,
    /// What View > Sort by checks.
    pub requested_sort: Option<RequestedSort>,
    pub show_hidden_files: bool,
    /// Whether the page draws the bar now (View > Show menu bar, the `appearance.menuBar`
    /// setting the frontend pushes).
    pub show_menu_bar: bool,
    pub active_pane: Pane,
    pub view_mode_left: ViewMode,
    pub view_mode_right: ViewMode,
    /// The view-mode pair's accelerators, which only the active pane's two items carry.
    pub view_mode_full_accel: Option<String>,
    pub view_mode_brief_accel: Option<String>,
    /// Every shortcut `update_menu_accelerator` pushed onto a row, by menu id, in the frontend's
    /// spelling (`""` unbinds). [`rebound_row_key`] decides what the row registers or shows.
    pub rebinds: HashMap<String, String>,
    pub is_pinned: bool,
    pub same_kind: Option<SameKindTarget>,
    pub undo_trash: Option<TrashUndoTarget>,
    /// Whether the user had a license key when the bar was built: "See license details" or
    /// "Enter license key…".
    pub has_existing_license: bool,
}

impl BarInputs {
    /// A bar as a fresh app builds it on `platform` (the desktop's `MenuState::default`): the
    /// explorer owns the menu, nothing is refused, no trash to undo, no closed tab to reopen, both
    /// panes on Brief, the left pane active.
    pub fn fresh(platform: Platform) -> Self {
        Self {
            explorer_menu_active: true,
            file_operations_blocked: false,
            open_terminal_here_enabled: true,
            reopen_closed_tab_enabled: false,
            refused: HashSet::new(),
            requested_sort: None,
            show_hidden_files: false,
            show_menu_bar: true,
            active_pane: Pane::Left,
            view_mode_left: ViewMode::Brief,
            view_mode_right: ViewMode::Brief,
            view_mode_full_accel: spec_accelerator(VIEW_MODE_FULL_LEFT_ID, platform).map(String::from),
            view_mode_brief_accel: spec_accelerator(VIEW_MODE_BRIEF_LEFT_ID, platform).map(String::from),
            rebinds: HashMap::new(),
            is_pinned: false,
            same_kind: None,
            undo_trash: None,
            has_existing_license: false,
        }
    }

    /// The enabled fold's inputs.
    pub fn item_inputs(&self) -> MenuItemInputs<'_> {
        MenuItemInputs {
            explorer_menu_active: self.explorer_menu_active,
            file_operations_blocked: self.file_operations_blocked,
            open_terminal_here_enabled: self.open_terminal_here_enabled,
            reopen_closed_tab_enabled: self.reopen_closed_tab_enabled,
            undo_trash_available: self.undo_trash.is_some(),
            refused: &self.refused,
        }
    }

    /// `update_menu_accelerator`'s push, stored the way the desktop's `commands/settings.rs`
    /// applies it: the view-mode pair's key goes to whichever pane is active (through the bar's
    /// floored door), every other row's is kept as pushed, by menu id.
    pub fn rebind(&mut self, command_id: &str, shortcut: &str, platform: Platform) {
        match command_id {
            "view.fullMode" => self.view_mode_full_accel = to_accelerator(shortcut, platform),
            "view.briefMode" => self.view_mode_brief_accel = to_accelerator(shortcut, platform),
            _ => {
                if let Some(menu_id) = command_id_to_menu_id(command_id) {
                    self.rebinds.insert(menu_id.to_string(), shortcut.to_string());
                }
            }
        }
    }

    fn view_mode(&self, pane: Pane) -> ViewMode {
        match pane {
            Pane::Left => self.view_mode_left,
            Pane::Right => self.view_mode_right,
        }
    }
}

/// `platform`'s bar as it stands under `inputs`: every menu and row, with its words, shown key,
/// and state.
pub fn bar_view(inputs: &BarInputs, platform: Platform) -> Vec<BarMenuView> {
    menus_on(platform)
        .map(|bar_menu| {
            let spec = &bar_menu.submenu;
            BarMenuView {
                // Top-level ids reach only macOS's native bar (`spec::menu`); a page names every
                // menu by the one id the spec gives it.
                id: spec.id.macos.unwrap_or_default().to_string(),
                title: label(spec.title, inputs),
                rows: rows(spec, inputs, platform),
            }
        })
        .collect()
}

fn rows(spec: &'static SubmenuSpec, inputs: &BarInputs, platform: Platform) -> Vec<Row> {
    let item_inputs = inputs.item_inputs();
    spec.entries_on(platform)
        .map(|entry| {
            let enabled = entry_enabled(entry, &item_inputs).unwrap_or(false);
            match entry {
                EntryKind::Item(item) => {
                    let (shortcut, aria_keyshortcuts) = hint_fields(key_hint(
                        item.id,
                        item.accelerator.on(platform),
                        item.display_accelerator.on(platform),
                        inputs,
                        platform,
                    ));
                    Row::Item {
                        id: item.id.to_string(),
                        label: item_label(item.id, item.label, inputs),
                        shortcut,
                        aria_keyshortcuts,
                        enabled,
                        icon: None,
                    }
                }
                EntryKind::Check(check) => {
                    let (checked, check_kind, hint) = match check.role {
                        CheckRole::ShowHiddenFiles => (
                            inputs.show_hidden_files,
                            CheckKind::Checkbox,
                            key_hint(check.id, check.accelerator.on(platform), None, inputs, platform),
                        ),
                        CheckRole::ViewMode(pane, mode) => {
                            // The pair's accelerator follows the active pane (`rebuild_view_mode_items`).
                            let accelerator = (pane == inputs.active_pane)
                                .then_some(match mode {
                                    ViewMode::Full => inputs.view_mode_full_accel.as_deref(),
                                    ViewMode::Brief => inputs.view_mode_brief_accel.as_deref(),
                                })
                                .flatten();
                            (
                                inputs.view_mode(pane) == mode,
                                CheckKind::Radio,
                                accelerator.map(|accelerator| KeyHint::registered(accelerator, platform)),
                            )
                        }
                        CheckRole::Sort(role) => (
                            role.is_on(inputs.requested_sort),
                            CheckKind::Radio,
                            key_hint(check.id, check.accelerator.on(platform), None, inputs, platform),
                        ),
                        CheckRole::ShowMenuBar => (
                            inputs.show_menu_bar,
                            CheckKind::Checkbox,
                            key_hint(check.id, check.accelerator.on(platform), None, inputs, platform),
                        ),
                    };
                    let (shortcut, aria_keyshortcuts) = hint_fields(hint);
                    Row::Check {
                        id: check.id.to_string(),
                        label: label(check.label, inputs),
                        shortcut,
                        aria_keyshortcuts,
                        enabled,
                        checked,
                        check_kind,
                    }
                }
                EntryKind::Submenu(nested) => Row::Submenu {
                    id: submenu_id(nested, platform),
                    label: label(nested.title, inputs),
                    enabled,
                    rows: rows(nested, inputs, platform),
                },
                EntryKind::Predefined(_, text) => Row::System {
                    label: label(*text, inputs),
                },
                EntryKind::Separator => Row::Separator,
            }
        })
        .collect()
}

/// A nested submenu's id: the spec's, else its title's catalog key.
fn submenu_id(spec: &SubmenuSpec, platform: Platform) -> String {
    spec.id
        .on(platform)
        .or_else(|| spec.title.catalog_key(false))
        .unwrap_or_default()
        .to_string()
}

/// A row's shortcut: a rebind first (as `MenuState.display_accelerators` overrides the spec), then
/// what `MENU_BAR` registers, then what it only shows.
fn key_hint(
    id: &str,
    registered: Option<&str>,
    displayed: Option<&str>,
    inputs: &BarInputs,
    platform: Platform,
) -> Option<KeyHint> {
    match inputs.rebinds.get(id) {
        Some(shortcut) => {
            let key = rebound_row_key(id, shortcut, platform);
            match (key.accelerator, key.display) {
                (Some(accelerator), _) => Some(KeyHint::registered(&accelerator, platform)),
                (None, Some(display)) => Some(KeyHint::displayed(&display, platform)),
                (None, None) => None,
            }
        }
        None => registered
            .map(|accelerator| KeyHint::registered(accelerator, platform))
            .or_else(|| displayed.map(|shortcut| KeyHint::displayed(shortcut, platform))),
    }
}

/// An item's words: the three live labels from their pushes, every other one from the spec.
fn item_label(id: &str, spec: Label, inputs: &BarInputs) -> RowLabel {
    match id {
        PIN_TAB_MENU_ID => pin_tab_label(inputs.is_pinned).into(),
        SELECT_SAME_KIND_ID => same_kind_label(inputs.same_kind.as_ref()).into(),
        UNDO_TRASH_ID => undo_trash_label(inputs.undo_trash.as_ref()).into(),
        _ => label(spec, inputs),
    }
}

fn label(spec: Label, inputs: &BarInputs) -> RowLabel {
    RowLabel::of(spec, inputs.has_existing_license)
}

/// One row's state, everything but its kind and children: what a change event carries for a row
/// whose answer moved.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RowState {
    /// The row's id ([`Row::id`]).
    pub id: String,
    pub label: RowLabel,
    pub shortcut: Option<String>,
    pub aria_keyshortcuts: Option<String>,
    pub enabled: bool,
    /// `None` for a row that isn't a check.
    pub checked: Option<bool>,
}

impl RowState {
    fn of(row: &Row) -> Option<Self> {
        Some(match row {
            Row::Item {
                id,
                label,
                shortcut,
                aria_keyshortcuts,
                enabled,
                icon: _,
            } => Self {
                id: id.clone(),
                label: label.clone(),
                shortcut: shortcut.clone(),
                aria_keyshortcuts: aria_keyshortcuts.clone(),
                enabled: *enabled,
                checked: None,
            },
            Row::Check {
                id,
                label,
                shortcut,
                aria_keyshortcuts,
                enabled,
                checked,
                ..
            } => Self {
                id: id.clone(),
                label: label.clone(),
                shortcut: shortcut.clone(),
                aria_keyshortcuts: aria_keyshortcuts.clone(),
                enabled: *enabled,
                checked: Some(*checked),
            },
            Row::Submenu { id, label, enabled, .. } => Self {
                id: id.clone(),
                label: label.clone(),
                shortcut: None,
                aria_keyshortcuts: None,
                enabled: *enabled,
                checked: None,
            },
            Row::Header { .. } | Row::System { .. } | Row::Separator => return None,
        })
    }
}

/// Every row of `menus`, depth-first, as its kind (for the shape) and its state.
fn flatten(menus: &[BarMenuView]) -> Vec<(String, Option<RowState>)> {
    fn walk(rows: &[Row], out: &mut Vec<(String, Option<RowState>)>) {
        for row in rows {
            let shape = match row {
                Row::Item { id, .. } => format!("item:{id}"),
                Row::Check { id, check_kind, .. } => format!("check:{id}:{check_kind:?}"),
                Row::Submenu { id, .. } => format!("submenu:{id}"),
                Row::Header { label } | Row::System { label } => format!("text:{label:?}"),
                Row::Separator => "separator".to_string(),
            };
            out.push((shape, RowState::of(row)));
            if let Row::Submenu { rows, .. } = row {
                walk(rows, out);
            }
        }
    }
    let mut out = Vec::new();
    for menu in menus {
        out.push((format!("menu:{}:{:?}", menu.id, menu.title), None));
        walk(&menu.rows, &mut out);
    }
    out
}

/// The rows whose state moved from `before` to `after`, or `None` when the two bars aren't the
/// same shape (a row added, removed, or moved, a menu retitled), which only a full answer
/// describes.
pub fn changed_rows(before: &[BarMenuView], after: &[BarMenuView]) -> Option<Vec<RowState>> {
    let before = flatten(before);
    let after = flatten(after);
    if before.len() != after.len() || before.iter().zip(&after).any(|(old, new)| old.0 != new.0) {
        return None;
    }
    Some(
        before
            .into_iter()
            .zip(after)
            .filter_map(|((_, old), (_, new))| match (old, new) {
                (Some(old), Some(new)) if old != new => Some(new),
                _ => None,
            })
            .collect(),
    )
}
