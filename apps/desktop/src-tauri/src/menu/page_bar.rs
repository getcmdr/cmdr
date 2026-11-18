//! The menu bar for a page that draws it (the in-window bar off macOS): the answer, the change
//! event, and a pick.
//!
//! Every row comes from `cmdr_menu::page::bar_view` over [`bar_inputs`], a read of the same
//! `MenuState` fields the native bar is written from, so the page bar and the native one can't
//! disagree and no row is decided here. `DETAILS.md` § "The bar a page draws" has the flow and
//! the ordering contract.
//!
//! - [`answer`] is `get_menu_bar`'s: the whole bar plus its revision. The first one subscribes the
//!   app to changes; before it, a push costs nothing extra (macOS never asks).
//! - [`notify_bar_changed`] runs after every push: it redraws, diffs against the last answer sent,
//!   and emits `menu-bar-changed` with the next revision and only the rows that moved.
//! - [`pick`] is a page's click: the same handler a native click reaches.

use std::sync::atomic::Ordering;

use cmdr_menu::click::{BarClick, bar_click};
use cmdr_menu::enabled::entry_enabled;
use cmdr_menu::page::{BarInputs, BarMenuView, RowState, bar_view, changed_rows};
use cmdr_menu::spec::{Pane, Platform};
use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event as _;

use crate::ignore_poison::IgnorePoison;

use super::{MenuState, cloned_handle};

/// The page bar's last answer and its revision. Lives in `MenuState.page_bar`.
#[derive(Debug, Default)]
pub struct BarFeed {
    /// Whether any page asked for the bar yet. Until one does, a push redraws nothing.
    subscribed: bool,
    /// Bumped by every answer that differs from the last one sent, event or command alike.
    revision: u64,
    /// The bar as of `revision`.
    last: Vec<BarMenuView>,
}

/// `get_menu_bar`'s answer: the whole bar, and the revision it stands at.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MenuBarAnswer {
    /// The revision this answer describes. A `menu-bar-changed` at `revision + 1` applies on top
    /// of it; one at or below it is already in here.
    // `u32` on the wire: specta refuses a 64-bit integer (JS numbers can't hold one exactly).
    pub revision: u32,
    pub menus: Vec<BarMenuView>,
}

/// `menu-bar-changed`: the page bar moved. Applies on top of the answer or event at
/// `revision - 1`; a gap means a page missed one and asks `get_menu_bar` again.
#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct MenuBarChanged {
    pub revision: u32,
    /// The rows whose state moved, by id. Empty when `menus` is set.
    pub rows: Vec<RowState>,
    /// The whole bar, when its shape moved (no row diff describes that).
    pub menus: Option<Vec<BarMenuView>>,
}

/// What a page's pick of a bar row did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum BarPickOutcome {
    /// The row's handler ran.
    Ran,
    /// The row is greyed out right now, or isn't on the bar: nothing ran, as a greyed native row
    /// can't be clicked.
    NotLive,
}

/// The bar's inputs, read from `MenuState` the way `apply_menu_item_states` and the native
/// builders read them. ❗ Reads locks only: never calls muda, so any thread may ask.
pub fn bar_inputs<R: Runtime>(menu_state: &MenuState<R>) -> BarInputs {
    BarInputs {
        explorer_menu_active: menu_state.explorer_menu_active.load(Ordering::Relaxed),
        file_operations_blocked: menu_state.file_operations_blocked.load(Ordering::Relaxed),
        open_terminal_here_enabled: menu_state.open_terminal_here_enabled.load(Ordering::Relaxed),
        reopen_closed_tab_enabled: menu_state.reopen_closed_tab_enabled.load(Ordering::Relaxed),
        refused: menu_state.commands_refused_over_dialog.lock_ignore_poison().clone(),
        requested_sort: *menu_state.requested_sort.lock_ignore_poison(),
        show_hidden_files: menu_state.show_hidden_checked.load(Ordering::Relaxed),
        active_pane: match menu_state.view_mode_active_pane.lock_ignore_poison().as_str() {
            "right" => Pane::Right,
            _ => Pane::Left,
        },
        view_mode_left: *menu_state.view_mode_left.lock_ignore_poison(),
        view_mode_right: *menu_state.view_mode_right.lock_ignore_poison(),
        view_mode_full_accel: menu_state.view_mode_full_accel.lock_ignore_poison().clone(),
        view_mode_brief_accel: menu_state.view_mode_brief_accel.lock_ignore_poison().clone(),
        rebinds: menu_state.pushed_shortcuts.lock_ignore_poison().clone(),
        is_pinned: menu_state.pin_tab_pinned.load(Ordering::Relaxed),
        same_kind: menu_state.same_kind_target.lock_ignore_poison().clone(),
        undo_trash: menu_state.undo_trash_target.lock_ignore_poison().clone(),
        has_existing_license: menu_state.has_existing_license.load(Ordering::Relaxed),
    }
}

/// The bar on this platform as it stands.
fn current<R: Runtime>(menu_state: &MenuState<R>) -> Vec<BarMenuView> {
    bar_view(&bar_inputs(menu_state), Platform::current())
}

/// `get_menu_bar`: the whole bar at its revision, and from now on a `menu-bar-changed` per move.
pub fn answer<R: Runtime>(menu_state: &MenuState<R>) -> MenuBarAnswer {
    let mut feed = menu_state.page_bar.lock_ignore_poison();
    let menus = current(menu_state);
    if !feed.subscribed || feed.last != menus {
        feed.revision += 1;
        feed.last = menus.clone();
    }
    feed.subscribed = true;
    MenuBarAnswer {
        revision: wire_revision(feed.revision),
        menus,
    }
}

/// Redraws the bar after a push and, when a row moved, emits `menu-bar-changed` to every window.
///
/// The redraw, the revision bump, and the emit happen under one lock, so two pushes racing from
/// different threads emit in revision order, and an answer can't slip between a bump and its
/// event: a page reading revision N from `get_menu_bar` has every change up to N in it.
pub fn notify_bar_changed<R: Runtime>(app: &AppHandle<R>) {
    let Some(menu_state) = app.try_state::<MenuState<R>>() else {
        return;
    };
    let mut feed = menu_state.page_bar.lock_ignore_poison();
    if !feed.subscribed {
        return;
    }
    let menus = current(&menu_state);
    if menus == feed.last {
        return;
    }
    let event = match changed_rows(&feed.last, &menus) {
        Some(rows) => MenuBarChanged {
            revision: wire_revision(feed.revision + 1),
            rows,
            menus: None,
        },
        None => MenuBarChanged {
            revision: wire_revision(feed.revision + 1),
            rows: Vec::new(),
            menus: Some(menus.clone()),
        },
    };
    feed.revision += 1;
    feed.last = menus;
    if let Err(e) = event.emit(app) {
        log::warn!(target: "menu", "couldn't emit menu-bar-changed: {e}");
    }
}

/// A revision as the wire carries it. A bar moves a few times a second at most, so `u32` lasts
/// decades; it wraps rather than panics, and a page treats a revision that doesn't follow as a
/// gap (it asks again).
fn wire_revision(revision: u64) -> u32 {
    (revision % (u64::from(u32::MAX) + 1)) as u32
}

/// A page's click on bar row `menu_id`, from the main bar or the viewer's: the same handler a
/// native click reaches (`handle_menu_pick`), after doing what a native click does by itself.
///
/// A native check item flips its own check before its handler hears of it, and the handler reads
/// the new state off it, so a pick flips Show hidden files first (both the stored check and the
/// item, where one is built). The view-mode and sort rows need nothing: their handlers write every
/// check from the stored state anyway.
pub fn pick(app: &AppHandle<tauri::Wry>, menu_id: &str) -> BarPickOutcome {
    let menu_state = app.state::<MenuState<tauri::Wry>>();
    if !is_live(&menu_state, menu_id) {
        return BarPickOutcome::NotLive;
    }
    if matches!(bar_click(menu_id, Platform::current()), Some(BarClick::ShowHiddenFiles)) {
        let flipped = !menu_state.show_hidden_checked.load(Ordering::Relaxed);
        menu_state.show_hidden_checked.store(flipped, Ordering::Relaxed);
        if let Some(item) = cloned_handle(&menu_state.show_hidden_files)
            && let Err(e) = item.set_checked(flipped)
        {
            log::warn!(target: "menu", "couldn't flip the native Show hidden files check: {e}");
        }
    }
    super::menu_handlers::handle_menu_pick(app, menu_id);
    BarPickOutcome::Ran
}

/// Whether `menu_id` is a row a page may pick right now: an enabled main-bar row, or one of the
/// viewer bar's (which the viewer page greys itself, from its own state).
fn is_live<R: Runtime>(menu_state: &MenuState<R>, menu_id: &str) -> bool {
    if cmdr_menu::page::viewer::is_viewer_bar_row(menu_id, Platform::current()) {
        return true;
    }
    let inputs = bar_inputs(menu_state);
    cmdr_menu::bar::spec_entry(menu_id, Platform::current())
        .and_then(|entry| entry_enabled(entry, &inputs.item_inputs()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{SameKindTarget, TrashUndoTarget};
    use cmdr_menu::sort::RequestedSort;
    use cmdr_menu::spec::ViewMode;
    use tauri::test::MockRuntime;

    /// A fresh `MenuState` reads as the bar a fresh app builds, the one the demo starts from too.
    #[test]
    fn a_fresh_menu_state_reads_as_a_fresh_bar() {
        let menu_state = MenuState::<MockRuntime>::default();
        assert_eq!(bar_inputs(&menu_state), BarInputs::fresh(Platform::current()));
    }

    /// Every field a push writes reaches the page bar's inputs, so the page draws what the native
    /// bar was told.
    #[test]
    fn every_pushed_fact_reaches_the_inputs() {
        let menu_state = MenuState::<MockRuntime>::default();
        menu_state.explorer_menu_active.store(false, Ordering::Relaxed);
        menu_state.file_operations_blocked.store(true, Ordering::Relaxed);
        menu_state.open_terminal_here_enabled.store(false, Ordering::Relaxed);
        menu_state.reopen_closed_tab_enabled.store(true, Ordering::Relaxed);
        menu_state
            .commands_refused_over_dialog
            .lock_ignore_poison()
            .insert("file.copy".to_string());
        let sort = RequestedSort {
            column: cmdr_listing::SortColumn::Size,
            order: cmdr_listing::SortOrder::Descending,
        };
        *menu_state.requested_sort.lock_ignore_poison() = Some(sort);
        menu_state.show_hidden_checked.store(true, Ordering::Relaxed);
        *menu_state.view_mode_active_pane.lock_ignore_poison() = "right".to_string();
        *menu_state.view_mode_left.lock_ignore_poison() = ViewMode::Full;
        *menu_state.view_mode_full_accel.lock_ignore_poison() = Some("Ctrl+F2".to_string());
        menu_state
            .pushed_shortcuts
            .lock_ignore_poison()
            .insert("new_tab".to_string(), "Ctrl+Alt+T".to_string());
        menu_state.pin_tab_pinned.store(true, Ordering::Relaxed);
        *menu_state.same_kind_target.lock_ignore_poison() = Some(SameKindTarget::AllFolders);
        *menu_state.undo_trash_target.lock_ignore_poison() = Some(TrashUndoTarget::One {
            name: "test_file_1.txt".to_string(),
        });
        menu_state.has_existing_license.store(true, Ordering::Relaxed);

        let mut expected = BarInputs::fresh(Platform::current());
        expected.explorer_menu_active = false;
        expected.file_operations_blocked = true;
        expected.open_terminal_here_enabled = false;
        expected.reopen_closed_tab_enabled = true;
        expected.refused.insert("file.copy".to_string());
        expected.requested_sort = Some(sort);
        expected.show_hidden_files = true;
        expected.active_pane = Pane::Right;
        expected.view_mode_left = ViewMode::Full;
        expected.view_mode_full_accel = Some("Ctrl+F2".to_string());
        expected.rebinds.insert("new_tab".to_string(), "Ctrl+Alt+T".to_string());
        expected.is_pinned = true;
        expected.same_kind = Some(SameKindTarget::AllFolders);
        expected.undo_trash = Some(TrashUndoTarget::One {
            name: "test_file_1.txt".to_string(),
        });
        expected.has_existing_license = true;
        assert_eq!(bar_inputs(&menu_state), expected);
    }

    /// The first answer subscribes and starts the count; an unchanged bar keeps its revision, and
    /// a moved one gets the next.
    #[test]
    fn an_answer_counts_revisions_only_when_the_bar_moves() {
        let menu_state = MenuState::<MockRuntime>::default();
        let first = answer(&menu_state);
        assert_eq!(first.revision, 1);
        assert_eq!(answer(&menu_state).revision, 1);
        menu_state.file_operations_blocked.store(true, Ordering::Relaxed);
        assert_eq!(answer(&menu_state).revision, 2);
    }

    /// A greyed row, or one the bar doesn't build, isn't a page's to pick.
    #[test]
    fn only_a_live_row_may_be_picked() {
        let menu_state = MenuState::<MockRuntime>::default();
        assert!(is_live(&menu_state, cmdr_menu::ids::FILE_COPY_ID));
        assert!(!is_live(&menu_state, cmdr_menu::ids::UNDO_TRASH_ID));
        assert!(!is_live(&menu_state, "no_such_row"));
        assert!(is_live(&menu_state, cmdr_menu::ids::VIEWER_CLOSE_ID));
        menu_state
            .commands_refused_over_dialog
            .lock_ignore_poison()
            .insert("file.copy".to_string());
        assert!(!is_live(&menu_state, cmdr_menu::ids::FILE_COPY_ID));
    }
}
