//! A right-click menu as a page draws it: the crate's [`ContextRow`]s with their words as catalog
//! keys, their shortcuts in the platform's spelling, and the OS-filled slots left out (a page has
//! no OS to ask for Open with, Share, the tags, the provider, or Services).
//!
//! Both page hosts draw through [`page_rows`]: the browser demo for every popup, and the desktop
//! for the in-page right-click menus (`apps/desktop/src-tauri/src/menu/page_context.rs`), so a
//! page menu says what the native one would.

use std::collections::HashSet;

use crate::context::click::{ContextClick, context_click};
use crate::context::{ContextLabel, ContextMenuShortcuts, ContextRow};
use crate::spec::Platform;

use super::{CheckKind, KeyHint, Row, RowLabel, hint_fields};

/// `rows` as the page draws them on `platform`, with each enabled row's id added to `live`: what
/// a pick may name, as only an enabled native row can be picked.
pub fn page_rows(
    rows: &[ContextRow],
    shortcuts: &ContextMenuShortcuts,
    platform: Platform,
    live: &mut HashSet<String>,
) -> Vec<Row> {
    rows.iter()
        .filter_map(|row| {
            Some(match row {
                ContextRow::Item(item) => {
                    if item.enabled {
                        live.insert(item.id.clone());
                    }
                    let (shortcut, aria_keyshortcuts) =
                        hint_fields(hint(item.shortcut.resolve(shortcuts, platform), platform));
                    Row::Item {
                        id: item.id.clone(),
                        label: label(&item.label),
                        shortcut,
                        aria_keyshortcuts,
                        enabled: item.enabled,
                    }
                }
                ContextRow::Check { item, checked } => {
                    if item.enabled {
                        live.insert(item.id.clone());
                    }
                    let (shortcut, aria_keyshortcuts) =
                        hint_fields(hint(item.shortcut.resolve(shortcuts, platform), platform));
                    Row::Check {
                        id: item.id.clone(),
                        label: label(&item.label),
                        shortcut,
                        aria_keyshortcuts,
                        enabled: item.enabled,
                        checked: *checked,
                        check_kind: check_kind(&item.id),
                    }
                }
                ContextRow::Submenu { id, label: text, rows } => Row::Submenu {
                    id: submenu_id(*id, text),
                    label: label(text),
                    enabled: true,
                    rows: page_rows(rows, shortcuts, platform, live),
                },
                ContextRow::Header(text) => Row::Header {
                    label: RowLabel::Text { text: text.clone() },
                },
                ContextRow::Separator => Row::Separator,
                // The OS fills these on a native menu; a page has no OS to ask.
                ContextRow::Slot(_) => return None,
            })
        })
        .collect()
}

/// A popup key is display text, never registered (`Shortcut::resolve` hands back Tauri's
/// spelling through the unfloored door), so it prints the way a registered one would.
fn hint(accelerator: Option<String>, platform: Platform) -> Option<KeyHint> {
    accelerator.map(|accelerator| KeyHint::registered(&accelerator, platform))
}

/// A header's date Sort by picks one column of three: a radio group. Every other context check
/// (a queue row's Select) toggles on its own.
fn check_kind(id: &str) -> CheckKind {
    match context_click(id) {
        Some(ContextClick::DateSort { .. }) => CheckKind::Radio,
        _ => CheckKind::Checkbox,
    }
}

/// A submenu's id: its own where it has one, else its label's catalog key.
fn submenu_id(id: Option<&str>, label: &ContextLabel) -> String {
    match (id, label) {
        (Some(id), _) => id.to_string(),
        (None, ContextLabel::Key(text)) => text.key.to_string(),
        (None, ContextLabel::Text(text)) => text.clone(),
    }
}

fn label(label: &ContextLabel) -> RowLabel {
    match label {
        ContextLabel::Key(text) => text.clone().into(),
        ContextLabel::Text(text) => RowLabel::Text { text: text.clone() },
    }
}
