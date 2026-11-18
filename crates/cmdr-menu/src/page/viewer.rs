//! The viewer window's bar as a page draws it.
//!
//! The native viewer bar is `apps/desktop/src-tauri/src/menu/menu_structure.rs`, which builds it
//! for macOS from OS items and Custom ones. A page has no OS items, so every row here is ours and
//! has an id a pick names: File > Close is [`VIEWER_CLOSE_ID`], and Cut and Paste are Custom on
//! every platform (macOS's own ids), forwarded to the search box like macOS's. The rows, their
//! order, labels, and fixed chords match `menu_structure.rs` (`viewer_test` in `bar_test.rs` pins
//! the Windows and Linux bar).
//!
//! Unlike the main bar, every input is the viewer page's own state (word wrap, its history, its
//! search box's focus), so a page asks with its state and gets rows back: no store, no event.

use serde::Deserialize;

use crate::bar::APP_MENU_TITLE;
use crate::context::popups::{viewer_copy_accelerator, viewer_select_all_accelerator};
use crate::enabled::viewer_text_edit_enabled;
use crate::ids::{
    APP_MENU_ID, EDIT_MENU_ID, FILE_MENU_ID, GO_MENU_ID, HELP_MENU_ID, VIEW_MENU_ID, VIEWER_CLOSE_ID,
    VIEWER_EDIT_COPY_ID, VIEWER_EDIT_CUT_ID, VIEWER_EDIT_PASTE_ID, VIEWER_GO_BACK_ID, VIEWER_GO_FORWARD_ID,
    VIEWER_SELECT_ALL_ID, VIEWER_WORD_WRAP_ID, WINDOW_MENU_ID,
};
use crate::spec::Platform;

use super::{BarMenuView, CheckKind, KeyHint, Row, RowLabel, hint_fields};

/// The viewer bar's Cut and Paste accelerators, as `menu_structure.rs` builds them on macOS and
/// as the OS's predefined items carry them elsewhere.
const fn cut_accelerator(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => "Cmd+X",
        Platform::Windows | Platform::Linux => "Ctrl+X",
    }
}

const fn paste_accelerator(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => "Cmd+V",
        Platform::Windows | Platform::Linux => "Ctrl+V",
    }
}

/// What the viewer page knows that its bar shows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewerBarInputs {
    /// View > Word wrap's check.
    pub word_wrap: bool,
    /// Go > Back is live.
    pub can_go_back: bool,
    /// Go > Forward is live.
    pub can_go_forward: bool,
    /// The search box holds keyboard focus, which is what makes Cut and Paste live.
    pub search_focused: bool,
}

/// The viewer bar on `platform` under `inputs`.
pub fn viewer_bar_view(inputs: ViewerBarInputs, platform: Platform) -> Vec<BarMenuView> {
    // The fold reads which viewer holds the focus; this page answers for itself.
    let text_edit = viewer_text_edit_enabled(inputs.search_focused.then_some(""));
    let mut menus = Vec::new();
    if platform == Platform::MacOs {
        menus.push(BarMenuView {
            id: APP_MENU_ID.to_string(),
            title: RowLabel::Text {
                text: APP_MENU_TITLE.to_string(),
            },
            rows: vec![
                system("menu.app.hide"),
                system("menu.app.hideOthers"),
                system("menu.app.showAll"),
                Row::Separator,
                system("menu.app.quit"),
            ],
        });
    }
    menus.push(BarMenuView {
        id: FILE_MENU_ID.to_string(),
        title: RowLabel::key("menu.bar.file"),
        rows: vec![item(
            VIEWER_CLOSE_ID,
            "menu.viewer.close",
            Some(close_accelerator(platform)),
            true,
            platform,
        )],
    });
    menus.push(BarMenuView {
        id: EDIT_MENU_ID.to_string(),
        title: RowLabel::key("menu.bar.edit"),
        rows: vec![
            item(
                VIEWER_EDIT_CUT_ID,
                "menu.edit.cut",
                Some(cut_accelerator(platform)),
                text_edit,
                platform,
            ),
            item(
                VIEWER_EDIT_COPY_ID,
                "menu.edit.copy",
                Some(viewer_copy_accelerator(platform)),
                true,
                platform,
            ),
            item(
                VIEWER_EDIT_PASTE_ID,
                "menu.edit.paste",
                Some(paste_accelerator(platform)),
                text_edit,
                platform,
            ),
            Row::Separator,
            item(
                VIEWER_SELECT_ALL_ID,
                "menu.select.all",
                Some(viewer_select_all_accelerator(platform)),
                true,
                platform,
            ),
        ],
    });
    menus.push(BarMenuView {
        id: VIEW_MENU_ID.to_string(),
        title: RowLabel::key("menu.bar.view"),
        rows: vec![Row::Check {
            id: VIEWER_WORD_WRAP_ID.to_string(),
            label: RowLabel::key("menu.viewer.wordWrap"),
            shortcut: None,
            aria_keyshortcuts: None,
            enabled: true,
            checked: inputs.word_wrap,
            check_kind: CheckKind::Checkbox,
        }],
    });
    // No accelerators, like Word wrap: the viewer's keydown owns ⌘[ / ⌘] through the registry.
    menus.push(BarMenuView {
        id: GO_MENU_ID.to_string(),
        title: RowLabel::key("menu.bar.go"),
        rows: vec![
            item(VIEWER_GO_BACK_ID, "menu.go.back", None, inputs.can_go_back, platform),
            item(
                VIEWER_GO_FORWARD_ID,
                "menu.go.forward",
                None,
                inputs.can_go_forward,
                platform,
            ),
        ],
    });
    if platform == Platform::MacOs {
        menus.push(BarMenuView {
            id: WINDOW_MENU_ID.to_string(),
            title: RowLabel::key("menu.bar.window"),
            rows: vec![system("menu.window.minimize"), system("menu.window.zoom")],
        });
        menus.push(BarMenuView {
            id: HELP_MENU_ID.to_string(),
            title: RowLabel::key("menu.bar.help"),
            rows: Vec::new(),
        });
    }
    menus
}

/// Whether `menu_id` is one of the rows `platform`'s viewer bar draws.
pub fn is_viewer_bar_row(menu_id: &str, platform: Platform) -> bool {
    viewer_bar_view(ViewerBarInputs::default(), platform)
        .iter()
        .flat_map(|menu| &menu.rows)
        .any(|row| row.id() == Some(menu_id))
}

/// The chord the OS's own Close window item carries: what a page's Close shows.
const fn close_accelerator(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => "Cmd+W",
        Platform::Windows | Platform::Linux => "Ctrl+W",
    }
}

fn item(id: &str, key: &str, accelerator: Option<&str>, enabled: bool, platform: Platform) -> Row {
    let (shortcut, aria_keyshortcuts) =
        hint_fields(accelerator.map(|accelerator| KeyHint::registered(accelerator, platform)));
    Row::Item {
        id: id.to_string(),
        label: RowLabel::key(key),
        shortcut,
        aria_keyshortcuts,
        enabled,
    }
}

fn system(key: &str) -> Row {
    Row::System {
        label: RowLabel::key(key),
    }
}
