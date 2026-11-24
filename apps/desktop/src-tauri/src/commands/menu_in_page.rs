//! The right-click menus' in-page twin, where the platform's `contextMenus` convention is
//! `inPage` (Windows): one request naming which `show_*` menu (`menu.rs`) it stands in for, with
//! that command's arguments, answered with the rows a page draws, and a pick that names the menu
//! it came from. The facts are stashed through `menu.rs`'s own helpers, so a pick finds
//! `MenuState` exactly as a native pick does. `menu/DETAILS.md` § "Right-click menus a page
//! draws".

use cmdr_menu::context::ContextMenuShortcuts;
use cmdr_menu::context::file::{FileMenuFacts, TargetFacts, file_menu};
use cmdr_menu::context::popups::{date_sort_menu, function_key_bar_menu, queue_row_menu, tab_menu, viewer_text_menu};
use cmdr_menu::spec::Platform;
use tauri::{AppHandle, Manager, Window};

use super::menu::{
    MenuAnchor, PaneContextMenuFacts, breadcrumb_rows, focus_for_context_menu, image_index_menu_state,
    network_host_rows, pane_facts, parent_row_rows, stash_file_context,
};
use crate::menu::{MenuState, SameKindTarget};

/// One right-click menu for a page to draw: the `show_*` command it stands in for, with that
/// command's own arguments (`menu` names which).
#[derive(Debug, serde::Deserialize, specta::Type)]
#[serde(tag = "menu", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum InPageContextMenuRequest {
    /// `show_file_context_menu`'s.
    File {
        path: String,
        filename: String,
        is_directory: bool,
        paths: Vec<String>,
        pane: PaneContextMenuFacts,
        target: crate::menu::ContextMenuTarget,
        shortcuts: ContextMenuShortcuts,
        anchor: Option<MenuAnchor>,
        /// The caller's live value, ❗ never the bar's debounced one (as `show_file_context_menu`).
        same_kind: Option<SameKindTarget>,
    },
    /// `show_breadcrumb_context_menu`'s.
    Breadcrumb {
        shortcuts: ContextMenuShortcuts,
        eject_volume_id: Option<String>,
        eject_volume_name: Option<String>,
        offer_eject_all: bool,
    },
    /// `show_parent_row_context_menu`'s.
    ParentRow {
        parent_path: String,
        anchor: Option<MenuAnchor>,
    },
    /// `show_tab_context_menu`'s.
    Tab {
        is_pinned: bool,
        can_close: bool,
        has_other_unpinned_tabs: bool,
    },
    /// `show_date_sort_menu`'s.
    DateSort {
        pane: crate::menu::PaneSide,
        requested: Option<cmdr_listing::SortColumn>,
        anchor: Option<MenuAnchor>,
    },
    /// `show_function_key_bar_context_menu`'s (none).
    FunctionKeyBar,
    /// `show_viewer_context_menu`'s.
    Viewer { has_selection: bool },
    /// `show_queue_row_context_menu`'s.
    QueueRow {
        entries: Vec<crate::menu::QueueRowMenuEntry>,
        shortcuts: ContextMenuShortcuts,
    },
    /// `show_network_host_context_menu`'s.
    NetworkHost {
        row_id: String,
        host: crate::network::NetworkHost,
        is_manual: bool,
        is_saved: bool,
        has_credentials: bool,
        anchor: Option<MenuAnchor>,
    },
}

/// A right-click menu as a page draws it: its rows, where it opens when the caller named a point
/// (`None`: at the pointer), and the token a pick from it names.
#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InPageContextMenu {
    pub token: u32,
    pub rows: Vec<cmdr_menu::page::Row>,
    pub anchor: Option<MenuAnchor>,
}

/// The in-page twin of every `show_*` popup command, where the platform's `contextMenus`
/// convention is `inPage` (Windows): stashes the same `MenuState` facts the native command does,
/// through the same helpers, and answers the rows to draw instead of popping a native menu up.
/// `menu/DETAILS.md` § "Right-click menus a page draws".
///
/// The file menu's OS-filled groups (Open with, Share, tags, the provider, Services) are left out,
/// as the browser demo leaves them out: they're macOS's, and macOS keeps native menus.
#[tauri::command]
#[specta::specta]
pub async fn open_context_menu_in_page(
    window: Window<tauri::Wry>,
    request: InPageContextMenuRequest,
) -> InPageContextMenu {
    // Read before anything is stashed, so the stash and the rows stay one synchronous step.
    let open_with = open_with_for(&request).await;
    let app = window.app_handle().clone();
    let state = app.state::<MenuState<tauri::Wry>>();
    let none = ContextMenuShortcuts::default;
    let (rows, shortcuts, anchor) = match request {
        InPageContextMenuRequest::File {
            path,
            filename,
            is_directory,
            paths,
            pane,
            target,
            shortcuts,
            anchor,
            same_kind,
        } => {
            let context_paths = if paths.is_empty() { vec![path.clone()] } else { paths };
            let count = context_paths.len();
            stash_file_context(&state, &path, &filename, context_paths, &pane.listing_id);
            let facts = FileMenuFacts {
                filename: &filename,
                is_directory,
                pane: pane_facts(&pane),
                image_index: image_index_menu_state(&path),
                target: TargetFacts {
                    count,
                    count_text: target.count_text.as_deref(),
                    size_text: target.size_text.as_deref(),
                },
                same_kind: same_kind.as_ref(),
            };
            (file_menu(&facts, Platform::current()), shortcuts, anchor)
        }
        InPageContextMenuRequest::Breadcrumb {
            shortcuts,
            eject_volume_id,
            eject_volume_name,
            offer_eject_all,
        } => (
            breadcrumb_rows(&state, eject_volume_id, eject_volume_name, offer_eject_all),
            shortcuts,
            None,
        ),
        InPageContextMenuRequest::ParentRow { parent_path, anchor } => {
            (parent_row_rows(&state, parent_path), none(), anchor)
        }
        InPageContextMenuRequest::Tab {
            is_pinned,
            can_close,
            has_other_unpinned_tabs,
        } => (tab_menu(is_pinned, can_close, has_other_unpinned_tabs), none(), None),
        InPageContextMenuRequest::DateSort {
            pane,
            requested,
            anchor,
        } => (date_sort_menu(pane, requested), none(), anchor),
        InPageContextMenuRequest::FunctionKeyBar => (function_key_bar_menu(), none(), None),
        InPageContextMenuRequest::Viewer { has_selection } => {
            (viewer_text_menu(has_selection, Platform::current()), none(), None)
        }
        InPageContextMenuRequest::QueueRow { entries, shortcuts } => (queue_row_menu(&entries), shortcuts, None),
        InPageContextMenuRequest::NetworkHost {
            row_id,
            host,
            is_manual,
            is_saved,
            has_credentials,
            anchor,
        } => (
            network_host_rows(&state, row_id, host, is_manual, is_saved, has_credentials),
            none(),
            anchor,
        ),
    };
    // The window the menu opens over is the one a pick acts in, as the native path makes it.
    focus_for_context_menu(&window);
    let (token, rows) = crate::menu::page_context::open(&state, &rows, &shortcuts, open_with.as_ref());
    InPageContextMenu { token, rows, anchor }
}

/// How long a file menu waits for Windows' "Open with" apps. A warm type answers from the cache
/// at once; a cold one is tens of milliseconds, its icons included. Past this the menu opens with
/// only "Choose another app", and the log says so.
#[cfg(target_os = "windows")]
const OPEN_WITH_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

/// Windows: the "Open with" apps for a file menu's rows, read on a Shell thread
/// (`file_system::open_with`). `None` for every other menu, a folder, and every other OS.
#[cfg(target_os = "windows")]
async fn open_with_for(request: &InPageContextMenuRequest) -> Option<crate::menu::page_context::PageOpenWith> {
    use crate::file_viewer::open_with_extract::any_needs_extraction;

    let InPageContextMenuRequest::File {
        path,
        paths,
        is_directory: false,
        ..
    } = request
    else {
        return None;
    };
    let rows = if paths.is_empty() {
        std::slice::from_ref(path)
    } else {
        paths.as_slice()
    };
    let native: Vec<std::path::PathBuf> = rows.iter().filter_map(|row| crate::app_text::native(row)).collect();
    let offer_another = !any_needs_extraction(&native);
    // Only the extensions matter to the Shell's handler lists, so an archive's insides ask
    // as they are, with no stand-in file.
    let asked = tokio::time::timeout(
        OPEN_WITH_WAIT,
        tokio::task::spawn_blocking(move || crate::file_system::open_with::compute_open_with_choices(native)),
    )
    .await;
    let choices = match asked {
        Ok(Ok(choices)) => choices,
        Ok(Err(e)) => {
            log::warn!(target: "open_with", "The Open with list didn't come back: {e}");
            crate::file_system::open_with::OpenWithChoices::default()
        }
        Err(_) => {
            log::info!(target: "open_with", "The Open with list took over {OPEN_WITH_WAIT:?}; opening the menu without it");
            crate::file_system::open_with::OpenWithChoices::default()
        }
    };
    Some(crate::menu::page_context::PageOpenWith { choices, offer_another })
}

#[cfg(not(target_os = "windows"))]
#[allow(clippy::unused_async, reason = "Windows' arm awaits the Shell")]
async fn open_with_for(_request: &InPageContextMenuRequest) -> Option<crate::menu::page_context::PageOpenWith> {
    None
}

/// A page's pick of `menu_id` from the right-click menu `open_context_menu_in_page` answered with
/// `token`: the handler a native pick reaches, or nothing when another menu has opened since.
#[tauri::command]
#[specta::specta]
pub fn pick_context_menu_item(
    app: AppHandle,
    window: tauri::WebviewWindow,
    token: u32,
    menu_id: String,
) -> crate::menu::page_context::ContextPickOutcome {
    crate::menu::page_context::pick(&app, token, &menu_id, window.label())
}
