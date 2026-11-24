//! Each OS's page-drawn slots. A platform seam (`docs/guides/platform-seams.md`).

use std::collections::HashSet;
use std::future::Future;

use cmdr_menu::context::Slot;
use cmdr_menu::page::Row;

use crate::menu::MenuState;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

/// The no-slot answer macOS and Linux share: they draw no right-click menu in the page.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod no_slots;

const _: fn(&mut PageSlots, Slot, &mut HashSet<String>) -> Vec<Row> = PageSlots::fill;
const _: fn(PageSlots, &MenuState<tauri::Wry>) = PageSlots::keep::<tauri::Wry>;
// An `async fn` doesn't coerce to a fn pointer, so this pin names its argument and output.
const _: fn() = || {
    fn resolves_to<T, F: Future<Output = T>>(_: F) {}
    resolves_to::<PageSlots, _>(read(None));
};

/// What a page menu's slots were filled from: [`read`] answers it before the menu's facts are
/// stashed, [`PageSlots::fill`] draws each slot it can (nothing for the rest), and
/// [`PageSlots::keep`] stores what a pick resolves through in `MenuState`, after the stash.
pub(crate) use imp::PageSlots;

/// Reads the slots for a file menu over `file_rows` (app path text; `None` for a folder and
/// every other menu), each read bounded by its own wait, so a slow answer leaves its slot short
/// rather than holding the menu.
pub(crate) use imp::read;
