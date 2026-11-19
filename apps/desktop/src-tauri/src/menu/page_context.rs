//! The right-click menus for a page that draws them (Windows' in-page menus, `contextMenus:
//! 'inPage'`): the rows a page draws, and a pick that names the menu it came from.
//!
//! Each `show_*` command has an in-page twin in `commands/menu_in_page.rs` (`open_context_menu_in_page`)
//! that stashes the same `MenuState` facts through the same helper, then answers here instead of
//! popping a native menu up. A pick then runs the handler a native pick reaches
//! (`handle_menu_pick`). `DETAILS.md` § "Right-click menus a page draws".
//!
//! A native menu is modal, so its pick always belongs to the stash it was built over. A page menu
//! isn't: another right-click can restash `MenuState` while it's still up. So every open (native
//! or page) moves a token, and a page's pick carries the token of the menu it came from: a stale
//! one acts on nothing.

use std::collections::HashSet;

use cmdr_menu::context::{ContextMenuShortcuts, ContextRow};
use cmdr_menu::page::Row;
use cmdr_menu::page::context::page_rows;
use cmdr_menu::spec::Platform;
use tauri::{AppHandle, Manager, Runtime};

use crate::ignore_poison::IgnorePoison;

use super::MenuState;

/// The right-click menu a page drew last. Lives in `MenuState.page_context`.
#[derive(Debug, Default)]
pub struct OpenPageMenu {
    /// Moved by every right-click menu that opens, native or page.
    token: u32,
    /// The ids the page menu at `token` offered enabled: what a pick may name. Empty while the
    /// last menu was native.
    live: HashSet<String>,
}

/// What a page's pick of a right-click row did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ContextPickOutcome {
    /// The row's handler ran.
    Ran,
    /// Another menu opened since this one (its facts are gone from `MenuState`), or the row
    /// wasn't offered enabled: nothing ran.
    Stale,
}

/// A native right-click menu is going up: any page menu still drawn is stale from here on.
pub fn note_native_popup<R: Runtime>(menu_state: &MenuState<R>) {
    let mut open = menu_state.page_context.lock_ignore_poison();
    open.token = open.token.wrapping_add(1);
    open.live.clear();
}

/// A page menu is going up over the facts just stashed: its rows, and the token a pick from it
/// names.
pub fn open<R: Runtime>(
    menu_state: &MenuState<R>,
    rows: &[ContextRow],
    shortcuts: &ContextMenuShortcuts,
) -> (u32, Vec<Row>) {
    let mut live = HashSet::new();
    let rows = page_rows(rows, shortcuts, Platform::current(), &mut live);
    let mut open = menu_state.page_context.lock_ignore_poison();
    open.token = open.token.wrapping_add(1);
    open.live = live;
    (open.token, rows)
}

/// Whether a pick of `menu_id` from the page menu at `token` may run.
fn may_pick<R: Runtime>(menu_state: &MenuState<R>, token: u32, menu_id: &str) -> bool {
    let open = menu_state.page_context.lock_ignore_poison();
    open.token == token && open.live.contains(menu_id)
}

/// A page's pick of `menu_id` from the menu at `token`: the handler a native pick reaches, or
/// nothing for a stale menu. One pick per menu, as a native menu closes on its pick.
pub fn pick(app: &AppHandle<tauri::Wry>, token: u32, menu_id: &str) -> ContextPickOutcome {
    let menu_state = app.state::<MenuState<tauri::Wry>>();
    if !may_pick(&menu_state, token, menu_id) {
        return ContextPickOutcome::Stale;
    }
    menu_state.page_context.lock_ignore_poison().live.clear();
    super::menu_handlers::handle_menu_pick(app, menu_id, super::menu_handlers::PickSource::NATIVE);
    ContextPickOutcome::Ran
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmdr_menu::context::popups::{function_key_bar_menu, tab_menu};
    use cmdr_menu::ids::{FUNCTION_KEY_BAR_HIDE_ID, TAB_CLOSE_OTHERS_ID, TAB_PIN_ID};
    use tauri::test::MockRuntime;

    /// The page draws the native menu's rows: same ids, same order, same enabled state.
    #[test]
    fn a_page_menu_draws_the_native_menus_rows() {
        let menu_state = MenuState::<MockRuntime>::default();
        let rows = tab_menu(false, true, false);
        let (_, drawn) = open(&menu_state, &rows, &ContextMenuShortcuts::default());
        let native: Vec<(String, bool)> = rows
            .iter()
            .filter_map(|row| match row {
                ContextRow::Item(item) => Some((item.id.clone(), item.enabled)),
                _ => None,
            })
            .collect();
        let page: Vec<(String, bool)> = drawn
            .iter()
            .filter_map(|row| match row {
                Row::Item { id, enabled, .. } => Some((id.clone(), *enabled)),
                _ => None,
            })
            .collect();
        assert_eq!(page, native);
    }

    /// A pick names the menu it came from: another menu opening (page or native) makes it stale,
    /// and a greyed row is never live.
    #[test]
    fn a_pick_from_a_menu_another_replaced_acts_on_nothing() {
        let menu_state = MenuState::<MockRuntime>::default();
        let (tab_token, _) = open(
            &menu_state,
            &tab_menu(false, true, false),
            &ContextMenuShortcuts::default(),
        );
        assert!(may_pick(&menu_state, tab_token, TAB_PIN_ID));
        // Close others is greyed with no other unpinned tab.
        assert!(!may_pick(&menu_state, tab_token, TAB_CLOSE_OTHERS_ID));

        let (bar_token, _) = open(&menu_state, &function_key_bar_menu(), &ContextMenuShortcuts::default());
        assert_ne!(bar_token, tab_token);
        assert!(!may_pick(&menu_state, tab_token, TAB_PIN_ID));
        assert!(may_pick(&menu_state, bar_token, FUNCTION_KEY_BAR_HIDE_ID));

        note_native_popup(&menu_state);
        assert!(!may_pick(&menu_state, bar_token, FUNCTION_KEY_BAR_HIDE_ID));
    }
}
