//! No page-drawn slots: the right-click menus are native here.

use std::collections::HashSet;

use cmdr_menu::context::Slot;
use cmdr_menu::page::Row;
use tauri::Runtime;

use crate::menu::MenuState;

/// Nothing read, nothing drawn, nothing kept.
pub(crate) struct PageSlots;

impl PageSlots {
    pub(crate) fn fill(&mut self, _slot: Slot, _live: &mut HashSet<String>) -> Vec<Row> {
        Vec::new()
    }

    pub(crate) fn keep<R: Runtime>(self, _menu_state: &MenuState<R>) {}
}

#[allow(
    clippy::unused_async,
    reason = "Windows' arm awaits the Shell; the seam pins one signature"
)]
pub(crate) async fn read(_file_rows: Option<Vec<String>>) -> PageSlots {
    PageSlots
}
