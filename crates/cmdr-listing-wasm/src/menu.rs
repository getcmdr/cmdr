//! The menu bar's host in the browser: the desktop's menu-state commands, answered by storing what
//! they push, and the visitor's own OS's bar drawn from `cmdr-menu` over that state.
//!
//! The desktop's `commands/menu_state.rs` (and `settings.rs`'s `update_menu_accelerator`) store
//! each push in `MenuState` and recompute the native items; [`DemoMenu`] stores the same pushes,
//! by the same command names and camelCase args, and answers [`DemoMenu::bar`] (what the page
//! draws) and [`DemoMenu::click`] (what a click sends) from the crate's own data and fold. So the
//! page's menu bar says what the native one would, with no menu logic in TS.

use std::sync::Mutex;

use cmdr_fs::ignore_poison::IgnorePoison as _;
use cmdr_menu::click::{BarClick, bar_click};
use cmdr_menu::enabled::entry_enabled;
use cmdr_menu::page::{BarInputs, bar_view};
use cmdr_menu::spec::{Pane, Platform, ViewMode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use cmdr_menu::page::{BarMenuView, Row, RowLabel};

/// Every command [`DemoMenu::invoke`] answers, by its Tauri name.
pub const MENU_COMMANDS: &[&str] = &[
    "activate_window_menu",
    "set_file_operations_blocked",
    "set_open_terminal_here_enabled",
    "set_reopen_closed_tab_enabled",
    "set_commands_refused_over_dialog",
    "update_menu_accelerator",
    "update_sort_menu",
    "update_view_mode_menu",
    "sync_menu_show_hidden",
    "sync_menu_show_menu_bar",
    "update_pin_tab_menu",
    "update_select_same_kind_menu",
    "update_undo_trash_menu",
    "update_menu_context",
];

/// What a click asks the page to do.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Click {
    /// Send `event` with `payload` to the window labeled `to`, as `handle_menu_event`'s `emit_to`.
    Emit {
        /// The Tauri event name.
        event: String,
        /// Its payload, as `bindings.ts` types it.
        payload: Value,
        /// The target window's label (`main`, `queue`, or the viewer a popup went up over).
        to: String,
    },
    /// Run the app command `command` with `args`, as `handle_menu_event` calls one directly
    /// (Add to favorites goes through `add_favorite`'s own gate).
    Invoke {
        /// The Tauri command name.
        command: String,
        /// Its camelCase args.
        args: Value,
    },
    /// Hand a native edit selector to whatever has focus outside the main window.
    NativeEdit {
        /// `cut`, `copy`, `paste`, or `selectAll`.
        action: String,
    },
    /// Close the focused window (Close tab while another window is in front).
    CloseFocusedWindow,
    /// Nothing: a disabled row, an OS item, or an id the bar doesn't build.
    Nothing,
}

/// The page's menu bar state.
pub struct DemoMenu {
    /// Whose bar and menus: the visitor's own OS (`lib/demo/platform.ts` reads it off the user agent).
    pub(crate) platform: Platform,
    model: Mutex<BarInputs>,
    /// What the last right-click menu went up over (`context_menu.rs`).
    pub(crate) context: Mutex<crate::context_menu::ContextModel>,
}

impl Default for DemoMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl DemoMenu {
    /// A Mac's bar as a fresh app builds it.
    pub fn new() -> Self {
        Self::for_platform(Platform::MacOs)
    }

    /// `platform`'s bar (and right-click menus) as a fresh app builds it.
    pub fn for_platform(platform: Platform) -> Self {
        Self {
            platform,
            model: Mutex::new(BarInputs::fresh(platform)),
            context: Mutex::new(crate::context_menu::ContextModel::default()),
        }
    }

    /// Whether [`DemoMenu::invoke`] answers `command`.
    pub fn handles(command: &str) -> bool {
        MENU_COMMANDS.contains(&command)
    }

    /// Stores one push, Tauri-style: `Ok` is the command's answer (all `null`), `Err` what the
    /// promise rejects with.
    pub fn invoke(&self, command: &str, args: Value) -> Result<Value, Value> {
        let mut m = self.model.lock_ignore_poison();
        match command {
            "activate_window_menu" => {
                let a: KindArgs = parse(command, args)?;
                match a.kind.as_str() {
                    "main" => m.explorer_menu_active = true,
                    "other" => m.explorer_menu_active = false,
                    // The viewer swaps in a bar of its own on desktop; the demo has no viewer yet.
                    "viewer" => {}
                    other => return Err(Value::String(format!("Unknown window menu kind: {other}"))),
                }
            }
            "set_file_operations_blocked" => m.file_operations_blocked = parse::<BlockedArgs>(command, args)?.blocked,
            "set_open_terminal_here_enabled" => {
                m.open_terminal_here_enabled = parse::<EnabledArgs>(command, args)?.enabled;
            }
            "set_reopen_closed_tab_enabled" => {
                m.reopen_closed_tab_enabled = parse::<EnabledArgs>(command, args)?.enabled;
            }
            "set_commands_refused_over_dialog" => {
                m.refused = parse::<RefusedArgs>(command, args)?.command_ids.into_iter().collect();
            }
            "update_menu_accelerator" => {
                let a: AcceleratorArgs = parse(command, args)?;
                m.rebind(&a.command_id, &a.shortcut, self.platform);
            }
            "update_sort_menu" => m.requested_sort = parse::<SortArgs>(command, args)?.sort,
            "update_view_mode_menu" => {
                let a: ViewModeArgs = parse(command, args)?;
                m.active_pane = match a.active_pane.as_str() {
                    "left" => Pane::Left,
                    "right" => Pane::Right,
                    other => return Err(Value::String(format!("Invalid active_pane: {other}"))),
                };
                let mode = |word: &str| {
                    ViewMode::from_wire(word).ok_or_else(|| Value::String(format!("Invalid view mode: {word}")))
                };
                m.view_mode_left = mode(&a.left_mode)?;
                m.view_mode_right = mode(&a.right_mode)?;
            }
            "sync_menu_show_hidden" => m.show_hidden_files = parse::<CheckedArgs>(command, args)?.checked,
            "sync_menu_show_menu_bar" => m.show_menu_bar = parse::<CheckedArgs>(command, args)?.checked,
            "update_pin_tab_menu" => m.is_pinned = parse::<PinnedArgs>(command, args)?.is_pinned,
            "update_select_same_kind_menu" => {
                m.same_kind = parse::<TargetArgs<cmdr_menu::labels::SameKindTarget>>(command, args)?.target
            }
            "update_undo_trash_menu" => {
                m.undo_trash = parse::<TargetArgs<cmdr_menu::labels::TrashUndoTarget>>(command, args)?.target
            }
            "update_menu_context" => {
                self.note_menu_context(args)?;
            }
            other => return Err(Value::String(format!("the demo's menu doesn't answer `{other}`"))),
        }
        Ok(Value::Null)
    }

    /// The bar as it stands: every menu and row, with its words, shortcut, and state, drawn
    /// by `cmdr_menu::page` as the desktop's in-window bar is.
    pub fn bar(&self) -> Vec<BarMenuView> {
        let mut menus = bar_view(&self.model.lock_ignore_poison(), self.platform);
        for menu in &mut menus {
            grey_what_a_page_cant_do(&mut menu.rows);
        }
        menus
    }

    /// What a click on `menu_id` asks for, applying what the click itself changes (a check item's
    /// own state), as `handle_menu_event` does.
    pub fn click(&self, menu_id: &str) -> Click {
        if PAGE_CANT_DO.contains(&menu_id) {
            return Click::Nothing;
        }
        let mut m = self.model.lock_ignore_poison();
        let enabled = cmdr_menu::bar::spec_entry(menu_id, self.platform)
            .and_then(|entry| entry_enabled(entry, &m.item_inputs()))
            .unwrap_or(false);
        let Some(click) = bar_click(menu_id, self.platform).filter(|_| enabled) else {
            return Click::Nothing;
        };
        let emit = |event: &str, payload: Value| Click::Emit {
            event: event.to_string(),
            payload,
            to: "main".to_string(),
        };
        match click {
            BarClick::Command { command_id, .. } => {
                emit("execute-command", json!({ "commandId": command_id, "road": "relay" }))
            }
            BarClick::ShowHiddenFiles => {
                m.show_hidden_files = !m.show_hidden_files;
                emit("settings-changed", json!({ "showHiddenFiles": m.show_hidden_files }))
            }
            BarClick::ViewMode { pane, mode } => {
                match pane {
                    Pane::Left => m.view_mode_left = mode,
                    Pane::Right => m.view_mode_right = mode,
                }
                emit("view-mode-changed", json!({ "mode": mode.wire(), "pane": pane.wire() }))
            }
            // The checks stay on the request until the frontend's sort lands and re-pushes.
            BarClick::Sort(check) => {
                let (action, value) = check.wire();
                emit("menu-sort", json!({ "action": action, "value": value }))
            }
            BarClick::Edit { command_id } if m.explorer_menu_active => {
                emit("execute-command", json!({ "commandId": command_id, "road": "relay" }))
            }
            BarClick::Edit { command_id } => Click::NativeEdit {
                action: native_action(command_id).to_string(),
            },
            BarClick::CloseTab if m.explorer_menu_active => {
                emit("execute-command", json!({ "commandId": "tab.close", "road": "relay" }))
            }
            BarClick::CloseTab => Click::CloseFocusedWindow,
        }
    }
}

/// Rows a page has nothing behind, drawn greyed and picking nothing:
/// - File > Quit (Exit on Windows), since a page can't quit its tab. It draws as an OS row, the way
///   a Mac visitor's app-menu Quit does (`Row::System`).
/// - View > Show menu bar (Windows and Linux visitors), since the demo's strip draws its own bar and
///   doesn't follow the setting: a live row would only flip its check (review of #1281). It stays a
///   greyed check, its state still following the push. Live once the demo draws the app's own bar
///   (task 63 step 11's remainder in `docs/specs/windows-port-plan.md`).
const PAGE_CANT_DO: [&str; 2] = [cmdr_menu::ids::QUIT_ID, cmdr_menu::ids::SHOW_MENU_BAR_ID];

fn grey_what_a_page_cant_do(rows: &mut [Row]) {
    for row in rows {
        match row {
            Row::Item { id, label, .. } if PAGE_CANT_DO.contains(&id.as_str()) => {
                *row = Row::System { label: label.clone() };
            }
            Row::Check { id, enabled, .. } if PAGE_CANT_DO.contains(&id.as_str()) => *enabled = false,
            Row::Submenu { rows, .. } => grey_what_a_page_cant_do(rows),
            _ => {}
        }
    }
}

fn native_action(command_id: &str) -> &'static str {
    match command_id {
        "edit.cut" => "cut",
        "edit.copy" => "copy",
        "edit.paste" => "paste",
        _ => "selectAll",
    }
}

pub(crate) fn parse<T: DeserializeOwned>(command: &str, args: Value) -> Result<T, Value> {
    serde_json::from_value(args).map_err(|e| Value::String(format!("invalid args for command {command}: {e}")))
}

#[derive(Deserialize)]
struct KindArgs {
    kind: String,
}

#[derive(Deserialize)]
struct BlockedArgs {
    blocked: bool,
}

#[derive(Deserialize)]
struct EnabledArgs {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefusedArgs {
    command_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceleratorArgs {
    command_id: String,
    shortcut: String,
}

#[derive(Deserialize)]
struct SortArgs {
    sort: Option<cmdr_menu::sort::RequestedSort>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewModeArgs {
    active_pane: String,
    left_mode: String,
    right_mode: String,
}

#[derive(Deserialize)]
struct CheckedArgs {
    checked: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PinnedArgs {
    is_pinned: bool,
}

#[derive(Deserialize)]
struct TargetArgs<T> {
    target: Option<T>,
}
