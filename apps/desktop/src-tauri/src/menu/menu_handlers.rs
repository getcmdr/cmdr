//! What happens when someone picks a menu item.
//!
//! `handle_menu_event` is the `.on_menu_event` dispatcher wired into the Tauri
//! builder: it maps a clicked item's ID to a command, a settings toggle, or a
//! native responder-chain action. The macOS post-construction passes live here
//! too (`cleanup_macos_menus`, `set_macos_menu_icons`, `set_display_accelerators`),
//! since they share this file's platform seam onto AppKit.
//!
//! The other two live-update lanes moved out: `accelerators.rs` for shortcut
//! strings, `view_mode_items.rs` for the per-pane view-mode items.

#[cfg(target_os = "macos")]
use tauri::Runtime;
use tauri::{AppHandle, Manager};

use std::sync::atomic::Ordering;

use crate::ignore_poison::IgnorePoison;
use crate::volume_broadcast::VolumeContextActionKind;
use crate::window_events::{ViewerEditActionKind, ViewerHistoryDirection};

use cmdr_menu::click::{BarClick, bar_click};
use cmdr_menu::context::click::{ContextClick, NetworkHostAction, VolumeAction, context_click};
use cmdr_menu::spec::{Pane, Platform};

use super::{
    CLOSE_TAB_ID, CommandScope, EDIT_COPY_ID, EDIT_CUT_ID, EDIT_PASTE_ID, MediaIndexFolderChoice,
    MediaIndexFolderExclusion, MenuSort, MenuState, SELECT_ALL_ID, SHOW_HIDDEN_FILES_ID, SettingsChanged,
    VIEW_SET_MODE_COMMAND_ID, VIEW_SHOW_HIDDEN_COMMAND_ID, VIEWER_CLOSE_ID, VIEWER_EDIT_COPY_ID, VIEWER_EDIT_CUT_ID,
    VIEWER_EDIT_PASTE_ID, VIEWER_GO_BACK_ID, VIEWER_GO_FORWARD_ID, VIEWER_SELECT_ALL_ID, VIEWER_WORD_WRAP_ID,
    ViewModeChanged, menu_id_to_command,
};

/// Removes macOS system-injected items from the Edit menu and registers the Help menu.
///
/// macOS AppKit automatically injects Writing Tools, AutoFill, Start Dictation, and Emoji & Symbols
/// into any menu it takes for an Edit menu. It also only shows the Help menu search field when a
/// menu is registered via `NSApplication.setHelpMenu:`. Both of these happen at the AppKit level
/// regardless of how the menu is constructed, so we fix them post-construction via native API
/// calls. Acts on whichever menu bar is installed (`app.menu()`), finding both menus by ID.
#[cfg(target_os = "macos")]
pub fn cleanup_macos_menus<R: Runtime>(app: &AppHandle<R>) {
    super::macos_appkit::cleanup_macos_menus(app);
}

/// Runs [`cleanup_macos_menus`] on the main thread, for callers running on a Tauri command thread.
///
/// `cleanup_macos_menus` (and `set_macos_menu_icons`) touch AppKit and must run on the main thread.
/// At startup `lib.rs` already runs in the `setup` hook on the main thread, so it calls them
/// directly; Tauri command handlers run on a worker thread, so they hop via `run_on_main_thread`.
/// Fire-and-forget: the cleanup is a UI tidy-up, so a failed hop only leaves the OS-injected Edit
/// items in place, never a broken state.
#[cfg(target_os = "macos")]
pub fn cleanup_macos_menus_from_command<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || cleanup_macos_menus(&handle)) {
        log::warn!(target: "menu", "Failed to dispatch macOS menu cleanup to the main thread: {e}");
    }
}

/// Sets SF Symbol icons on menu items post-construction via native AppKit API.
///
/// Tauri's menu API doesn't support SF Symbols, so we walk the NSMenu hierarchy after
/// construction and call `NSImage(systemSymbolName:accessibilityDescription:)` + `setImage:`
/// on each item. Which item gets which symbol is keyed by menu item ID; the ID is resolved to the
/// item's current title only to find it on the AppKit side, which knows no other index.
#[cfg(target_os = "macos")]
pub fn set_macos_menu_icons<R: Runtime>(app: &AppHandle<R>) {
    super::macos_appkit::set_macos_menu_icons(app);
}

/// Runs [`set_macos_menu_icons`] on the main thread, for callers running on a Tauri command thread.
///
/// Same fire-and-forget contract as [`cleanup_macos_menus_from_command`]: a failed hop costs icons,
/// never correctness.
#[cfg(target_os = "macos")]
pub fn set_macos_menu_icons_from_command<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || set_macos_menu_icons(&handle)) {
        log::warn!(target: "menu", "Failed to dispatch macOS menu icons to the main thread: {e}");
    }
}

/// Draws the display-only accelerators on the installed menu bar, post-construction via AppKit.
///
/// The counterpart of [`set_macos_menu_icons`] for the shortcuts a menu item can only SHOW
/// (`menu_spec::ItemSpec::display_accelerator`), and it has to run everywhere that one does:
/// neither an attributed title nor an image survives a fresh `NSMenuItem`.
#[cfg(target_os = "macos")]
pub fn set_display_accelerators<R: Runtime>(app: &AppHandle<R>, menu_state: &MenuState<R>) {
    super::display_accelerators::set_display_accelerators(app, menu_state);
}

/// Runs [`set_display_accelerators`] on the main thread, for callers on a Tauri command thread.
///
/// Same fire-and-forget contract as [`set_macos_menu_icons_from_command`]: a failed hop costs a
/// glyph until the next menu-bar swap, never correctness. Reads `MenuState` inside the hop, which
/// is safe because every caller of this variant runs long after `app.manage`.
#[cfg(target_os = "macos")]
pub fn set_display_accelerators_from_command<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let dispatched = app.run_on_main_thread(move || {
        let menu_state = handle.state::<MenuState<R>>();
        set_display_accelerators(&handle, &menu_state);
    });
    if let Err(e) = dispatched {
        log::warn!(target: "menu", "Failed to dispatch the menu bar's display accelerators to the main thread: {e}");
    }
}

/// A native text-editing selector a Custom menu item forwards to the first responder.
///
/// A typed variant rather than the selector's name as a string, so the id → selector decision
/// can be tested on every platform while `sel!` stays macOS-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeEditSelector {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// The native selector menu item `menu_id` forwards, or `None` when it forwards none.
///
/// Both bars are in here. The main bar's Edit / Select items take this lane only outside the
/// main window (`handle_menu_event`), while the viewer bar's Cut and Paste always do: the
/// viewer's search box is the only editable field in that window. ❗ The viewer bar's Copy and
/// Select all are deliberately absent — they act on the viewer's own offset-based selection,
/// which no native selector can reach (`viewer_edit_action_for`).
pub(crate) fn native_edit_selector_for(menu_id: &str) -> Option<NativeEditSelector> {
    match menu_id {
        EDIT_CUT_ID | VIEWER_EDIT_CUT_ID => Some(NativeEditSelector::Cut),
        EDIT_COPY_ID => Some(NativeEditSelector::Copy),
        EDIT_PASTE_ID | VIEWER_EDIT_PASTE_ID => Some(NativeEditSelector::Paste),
        SELECT_ALL_ID => Some(NativeEditSelector::SelectAll),
        _ => None,
    }
}

/// Which windows hold focus, as far as the Edit rows care.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FocusedWindows {
    pub main: bool,
    pub queue: bool,
}

pub(crate) fn focused_windows(app: &AppHandle<tauri::Wry>) -> FocusedWindows {
    let focused = |label: &str| {
        app.get_webview_window(label)
            .is_some_and(|w| w.is_focused().unwrap_or(false))
    };
    FocusedWindows {
        main: focused("main"),
        queue: focused("queue"),
    }
}

/// Where an Edit row's pick goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditRoad {
    /// The main window decides between file and text semantics (`execute-command`).
    MainCommand(&'static str),
    /// The queue selects its rows, never its text.
    QueueSelectAll,
    /// The focused window's focused text field does the edit: on macOS the responder chain
    /// (`super::focused_field_edit`), elsewhere the page itself (`page_bar::pick` hands it back).
    FieldEdit(NativeEditSelector),
}

/// Where Edit row `menu_id`'s pick goes with `focus`, or `None` for a row that isn't one.
///
/// - The viewer bar's Cut and Paste always edit its search box, the only editable thing in that
///   window (and why they're greyed while it lacks focus). ❗ Never the main bar's rule: that one
///   asks whether the MAIN window is focused, and a viewer pick would answer no and land in the same
///   place by accident rather than by rule.
/// - The main bar's four go through `execute-command` in the main window, so the frontend decides
///   between file and text semantics. Select all in the queue checks its rows. Anywhere else the
///   focused field does the text edit: without it, ⌘A would be dead in a Settings field on macOS
///   (the accelerator fires before the webview sees the key, and the file-scoped guard drops it).
pub(crate) fn edit_road(menu_id: &str, focus: FocusedWindows) -> Option<EditRoad> {
    if menu_id == VIEWER_EDIT_CUT_ID || menu_id == VIEWER_EDIT_PASTE_ID {
        return native_edit_selector_for(menu_id).map(EditRoad::FieldEdit);
    }
    let command_id = match menu_id {
        EDIT_CUT_ID => "edit.cut",
        EDIT_COPY_ID => "edit.copy",
        EDIT_PASTE_ID => "edit.paste",
        SELECT_ALL_ID => "selection.selectAll",
        _ => return None,
    };
    if focus.main {
        return Some(EditRoad::MainCommand(command_id));
    }
    if menu_id == SELECT_ALL_ID && focus.queue {
        return Some(EditRoad::QueueSelectAll);
    }
    native_edit_selector_for(menu_id).map(EditRoad::FieldEdit)
}

/// Runs an Edit row's `road`.
fn run_edit_road(app: &AppHandle<tauri::Wry>, road: EditRoad) {
    use tauri_specta::Event as _;
    match road {
        EditRoad::MainCommand(command_id) => {
            let _ = crate::window_events::ExecuteCommand::relay(command_id).emit_to(app, "main");
        }
        // On macOS the queue's own keydown never sees ⌘A (the native accelerator fires first), so
        // this event is the key's only road there. Off macOS no bar wears the key, and the queue's
        // keydown runs it (`routes/queue/+page.svelte`); a page bar's pick still lands here.
        EditRoad::QueueSelectAll => {
            if let Err(e) = crate::window_events::QueueSelectAll.emit_to(app, "queue") {
                log::warn!(target: "menu", "failed to emit queue-select-all: {e}");
            }
        }
        // macOS sends the selector down the responder chain, as a Predefined item would. Elsewhere
        // only a page's pick reaches an Edit row, and `page_bar::pick` hands the edit back to the page
        // before it gets here; the seam's Windows and Linux arms run nothing.
        EditRoad::FieldEdit(selector) => super::focused_field_edit::run_in_focused_window(app, selector),
    }
}

/// The native selector the viewer page asks for when the view on screen keeps its selection
/// where only the responder chain reaches it: the PDF embed (`viewer_forward_edit_action_to_page`).
pub(crate) fn native_edit_selector_for_viewer_page(action: ViewerEditActionKind) -> NativeEditSelector {
    match action {
        ViewerEditActionKind::Copy => NativeEditSelector::Copy,
        ViewerEditActionKind::SelectAll => NativeEditSelector::SelectAll,
    }
}

/// Sends `selector` to the key window's first responder, exactly like a Predefined menu item.
#[cfg(target_os = "macos")]
pub(crate) fn send_native_edit_selector(selector: NativeEditSelector) {
    use objc2::sel;
    use objc2_app_kit::NSApplication;

    let selector = match selector {
        NativeEditSelector::Cut => sel!(cut:),
        NativeEditSelector::Copy => sel!(copy:),
        NativeEditSelector::Paste => sel!(paste:),
        NativeEditSelector::SelectAll => sel!(selectAll:),
    };

    let mtm = objc2::MainThreadMarker::new().expect("send_native_edit_selector must be called from the main thread");
    let ns_app = NSApplication::sharedApplication(mtm);

    // sendAction:to:from: with nil `to` sends to the first responder, exactly like
    // PredefinedMenuItems do internally. This lets WKWebView handle text clipboard natively.
    // SAFETY: `ns_app` is the live `sharedApplication` singleton; `sendAction:to:from:` takes
    // `(SEL, id, id)` — `selector` is one of the responder-chain editing selectors matched above, and
    // both `to`/`from` are nil (routes to the first responder). Returns `BOOL`, decoded as `bool`. On
    // the main thread (the `MainThreadMarker` above asserts it), as AppKit requires.
    unsafe {
        let _: bool = objc2::msg_send![
            &ns_app,
            sendAction: selector,
            to: std::ptr::null::<objc2::runtime::AnyObject>(),
            from: std::ptr::null::<objc2::runtime::AnyObject>(),
        ];
    }
}

/// The label of the viewer window that currently has focus, if one does.
///
/// The viewer bar is app-level on macOS and shared by every open viewer, so an item in it names
/// no window of its own: the focused one is the only honest answer. Viewer windows are labeled
/// `viewer-<n>` (`commands/file_viewer.rs`, `capabilities/viewer.json`).
fn focused_viewer_label(app: &AppHandle<tauri::Wry>) -> Option<String> {
    app.webview_windows()
        .into_iter()
        .find(|(label, window)| label.starts_with("viewer-") && window.is_focused().unwrap_or(false))
        .map(|(label, _)| label)
}

/// The viewer Edit-menu action a clicked item id names, or `None` when the id isn't one of them.
fn viewer_edit_action_for(menu_id: &str) -> Option<ViewerEditActionKind> {
    match menu_id {
        VIEWER_EDIT_COPY_ID => Some(ViewerEditActionKind::Copy),
        VIEWER_SELECT_ALL_ID => Some(ViewerEditActionKind::SelectAll),
        _ => None,
    }
}

/// The viewer Go-menu step a clicked item id names, or `None` when the id isn't one of them.
fn viewer_history_direction_for(menu_id: &str) -> Option<ViewerHistoryDirection> {
    match menu_id {
        VIEWER_GO_BACK_ID => Some(ViewerHistoryDirection::Back),
        VIEWER_GO_FORWARD_ID => Some(ViewerHistoryDirection::Forward),
        _ => None,
    }
}

/// Dispatches a global-menu click to the right window or frontend command.
///
/// Wired into the Tauri builder as `.on_menu_event(menu::handle_menu_event)`. Most items flow
/// through the unified `menu_id_to_command` mapping at the bottom and emit `execute-command` to
/// the main window; the blocks above it are the exceptions that need direct emits, per-pane
/// state syncing, focus-routed clipboard handling, or native macOS panels.
pub fn handle_menu_event(app: &AppHandle<tauri::Wry>, event: tauri::menu::MenuEvent) {
    handle_menu_pick(app, event.id().as_ref(), PickSource::NATIVE);
}

/// Where a menu pick came from, which decides how a plain command leaves for the main window and
/// which window a window-bound row (Edit, Close, the viewer's rows) acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PickSource {
    /// The road the command takes: `Relay` for a native menu and any key, `BarClick` for a page bar
    /// row the person clicked.
    pub road: crate::window_events::CommandRoad,
    /// The window the pick is about.
    pub window: PickWindow,
}

/// The window a pick came from. A page's pick names its own window, which is surer than focus:
/// focus can lag a window switch, and an X session with no window manager (the Linux E2E lane)
/// reports it wrong. Regression anchor (review of #1281): a Ctrl+V in the queue window, read
/// against a stale "main is focused", pasted files into the main window's pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PickWindow {
    /// A native menu, whose event names no window: the focused one stands in.
    Focused,
    /// The page that drew the row (or fired its chord), by window label (`main`, `queue`,
    /// `viewer-<n>`, ...).
    Page(String),
}

impl PickSource {
    /// A native menu's click or accelerator.
    pub const NATIVE: Self = Self {
        road: crate::window_events::CommandRoad::Relay,
        window: PickWindow::Focused,
    };

    /// A page's pick in the window labeled `label`, down `road`.
    pub fn page(road: crate::window_events::CommandRoad, label: &str) -> Self {
        Self {
            road,
            window: PickWindow::Page(label.to_string()),
        }
    }

    /// Whether the pick came from the main window. A file-scoped command runs only from there.
    fn is_from_main_window(&self, app: &AppHandle<tauri::Wry>) -> bool {
        match &self.window {
            PickWindow::Page(label) => label == "main",
            PickWindow::Focused => app
                .get_webview_window("main")
                .is_some_and(|w| w.is_focused().unwrap_or(false)),
        }
    }

    /// The windows an Edit row asks about (`edit_road`): the page's own, or focus for a native pick.
    pub fn windows(&self, app: &AppHandle<tauri::Wry>) -> FocusedWindows {
        match &self.window {
            PickWindow::Page(label) => windows_of_page(label),
            PickWindow::Focused => focused_windows(app),
        }
    }

    /// The viewer window a viewer row acts on: the page's own, or the focused viewer for a native pick.
    fn viewer_label(&self, app: &AppHandle<tauri::Wry>) -> Option<String> {
        match &self.window {
            PickWindow::Page(label) => label.starts_with("viewer-").then(|| label.clone()),
            PickWindow::Focused => focused_viewer_label(app),
        }
    }
}

/// What `edit_road` hears for a pick from the page in window `label`.
pub(crate) fn windows_of_page(label: &str) -> FocusedWindows {
    FocusedWindows {
        main: label == "main",
        queue: label == "queue",
    }
}

/// What a pick of menu item `id` does, from a native menu (`handle_menu_event`) or from a page
/// that draws its menus itself (`page_bar::pick`, `page_context::pick`): one handler, so the two
/// roads can't drift. A page's pick has already done what a native click does by itself (a check
/// item's own flip). `source` says how a plain command (the unified dispatch at the bottom) leaves.
pub(crate) fn handle_menu_pick(app: &AppHandle<tauri::Wry>, id: &str, source: PickSource) {
    // === A context-menu row's own lane: what it asks for comes off its id (`cmdr_menu::context::click`),
    // the same answer the browser demo's context menu gets. A plain command falls through to the
    // unified dispatch at the bottom, with the bar's.
    match context_click(id) {
        Some(ContextClick::Command { .. }) | None => {}
        // A row only the context menu carries: the main window hears it came from a right-click.
        Some(ContextClick::RowCommand { command_id, scope }) => {
            let source = PickSource {
                road: crate::window_events::CommandRoad::ContextMenu,
                ..source
            };
            emit_command(app, command_id, scope, &source);
            return;
        }
        Some(click) => {
            dispatch_context_click(app, click, &source);
            return;
        }
    }

    // === CheckMenuItem exceptions: sync checked state and emit directly ===
    // These must NOT go through "execute-command", as that would double-toggle.
    //
    // ❗ They're also the one place the main window's dialog gate is applied in Rust. The item has
    // already toggled itself by the time we hear of the click, and the frontend can't untoggle it:
    // show hidden never reaches the dispatch core at all, and a refused `view.setMode` would leave
    // the check on a mode the pane isn't in. So a refused click puts the check back and stops here.
    // A disabled item's accelerator still fires, so the greying alone doesn't cover it.
    if id == SHOW_HIDDEN_FILES_ID {
        let menu_state = app.state::<MenuState<tauri::Wry>>();
        let check_item = super::cloned_handle(&menu_state.show_hidden_files);
        // The click's new state: the native item's own (it toggled itself), or the stored check a
        // page's pick flipped where no item is built.
        let new_state = match &check_item {
            Some(item) => item.is_checked().unwrap_or(true),
            None => menu_state.show_hidden_checked.load(Ordering::Relaxed),
        };
        let refused = menu_state.refuses_over_dialog(VIEW_SHOW_HIDDEN_COMMAND_ID);
        // A refused click puts the check back.
        let settled = if refused { !new_state } else { new_state };
        if refused && let Some(item) = &check_item {
            let _ = item.set_checked(settled);
        }
        menu_state.show_hidden_checked.store(settled, Ordering::Relaxed);
        super::page_bar::notify_bar_changed(app);
        if !refused {
            use tauri_specta::Event as _;
            let _ = SettingsChanged {
                show_hidden_files: new_state,
            }
            .emit_to(app, "main");
        }
        return;
    }
    // What the bar's own rows ask for comes off the row (`cmdr_menu::click`), the same answer the
    // browser demo's bar gets.
    let bar = bar_click(id, Platform::current());
    if let Some(BarClick::ViewMode { pane, mode }) = bar {
        // Per-pane view mode click. Sync the affected pane's pair (the muda click
        // already toggled the clicked item, so unchecking the sibling is enough),
        // store the new mode in MenuState, and notify the frontend with the target
        // pane so it can update without changing focus.
        let menu_state = app.state::<MenuState<tauri::Wry>>();
        if menu_state.refuses_over_dialog(VIEW_SET_MODE_COMMAND_ID) {
            // Re-sync every check from the stored modes, which undoes muda's toggle.
            let _ = super::view_mode_items::sync_view_mode_check_states(&menu_state);
            return;
        }
        match pane {
            Pane::Left => *menu_state.view_mode_left.lock_ignore_poison() = mode,
            Pane::Right => *menu_state.view_mode_right.lock_ignore_poison() = mode,
        }
        let _ = super::view_mode_items::sync_view_mode_check_states(&menu_state);
        super::page_bar::notify_bar_changed(app);
        use tauri_specta::Event as _;
        let _ = ViewModeChanged {
            mode: mode.wire().to_string(),
            pane: pane.wire().to_string(),
        }
        .emit_to(app, "main");
        return;
    }

    // === Close-tab exception: close focused non-main window, or emit tab.close ===
    if id == CLOSE_TAB_ID {
        if source.is_from_main_window(app) {
            use tauri_specta::Event as _;
            let _ = crate::window_events::ExecuteCommand::relay("tab.close").emit_to(app, "main");
        } else if let PickWindow::Page(label) = &source.window {
            // A bar-less window fired the row's chord (Ctrl+W): that window closes, whatever has focus.
            if let Some(window) = app.get_webview_window(label)
                && let Err(e) = window.close()
            {
                // allowed-log-private: a window label Cmdr mints (`queue`, `settings`, `viewer-<n>`), no user value
                log::warn!(target: "menu", "couldn't close the {label} window from its Close key: {e}");
            }
        } else {
            for (_label, window) in app.webview_windows() {
                if window.is_focused().unwrap_or(false) {
                    let _ = window.close();
                    break;
                }
            }
        }
        return;
    }

    // === Viewer File > Close as a page draws it: close the viewer it came from ===
    // The native bar's Close is the OS's own item and never reaches here.
    if id == VIEWER_CLOSE_ID {
        let window = source
            .viewer_label(app)
            .and_then(|label| app.get_webview_window(&label));
        match window.map(|window| window.close()) {
            Some(Ok(())) => {}
            Some(Err(e)) => log::warn!(target: "menu", "couldn't close the viewer from its bar: {e}"),
            None => log::warn!(target: "menu", "Viewer Close picked with no viewer focused, ignoring"),
        }
        return;
    }

    // === Viewer word wrap: emit to the focused viewer window ===
    if id == VIEWER_WORD_WRAP_ID {
        if let Some(label) = source.viewer_label(app) {
            use tauri_specta::Event as _;
            let _ = crate::window_events::ViewerWordWrapToggled.emit_to(app, &label);
        }
        return;
    }

    // === Viewer Edit > Copy / Select all: emit to the focused viewer window ===
    // The viewer runs both itself, over its own offset-based selection; see
    // `window_events::ViewerEditAction` for why these can't be native selectors.
    if let Some(action) = viewer_edit_action_for(id) {
        let Some(label) = source.viewer_label(app) else {
            log::warn!(target: "menu", "Viewer Edit item {id} clicked with no viewer focused, ignoring");
            return;
        };
        use tauri_specta::Event as _;
        let _ = crate::window_events::ViewerEditAction { action }.emit_to(app, &label);
        return;
    }

    // === Viewer Go > Back / Forward: emit to the focused viewer window, which owns its history ===
    if let Some(direction) = viewer_history_direction_for(id) {
        let Some(label) = source.viewer_label(app) else {
            log::warn!(target: "menu", "Viewer Go item {id} clicked with no viewer focused, ignoring");
            return;
        };
        use tauri_specta::Event as _;
        let _ = crate::window_events::ViewerHistoryAction { direction }.emit_to(app, &label);
        return;
    }

    // === The Edit rows: file semantics in the main window, the focused field's text elsewhere ===
    // One decision for both bars and every window (`edit_road`), which a page's pick asks too.
    // Focus is read only for an Edit row: two window lookups per pick otherwise.
    if native_edit_selector_for(id).is_some()
        && let Some(road) = edit_road(id, source.windows(app))
    {
        run_edit_road(app, road);
        return;
    }

    // === Sort items: emit menu-sort directly (frontend has a dedicated listener) ===
    // They're check items, and muda has already toggled the clicked one by now. Put every check
    // back on the stored request first: the check moves when the frontend's sort lands and it
    // re-pushes (`update_sort_menu`), and a sort the frontend refuses (over a dialog) leaves it be.
    if let Some(BarClick::Sort(check)) = bar {
        super::sync_sort_check_states(&app.state::<MenuState<tauri::Wry>>());
        let (action, value) = check.wire();
        use tauri_specta::Event as _;
        let _ = MenuSort {
            action: action.to_string(),
            value: value.to_string(),
        }
        .emit_to(app, "main");
        return;
    }

    // === Open with submenu: dynamic IDs prefix-routed before unified dispatch ===
    // Items have IDs like `open-with:com.apple.Xcode`, too dynamic to enumerate
    // in `menu_id_to_command`. We resolve the bundle ID back to an app path via
    // `MenuState.context.open_with_apps` and call the launch helper directly.
    #[cfg(target_os = "macos")]
    if let Some(bundle_id) = id.strip_prefix(super::open_with::OPEN_WITH_ID_PREFIX) {
        let menu_state = app.state::<MenuState<tauri::Wry>>();
        let ctx = menu_state.context.lock_ignore_poison();
        let app_path = ctx.open_with_apps.get(bundle_id).cloned();
        let rows = ctx.paths.clone();
        let listing_id = ctx.listing_id.clone();
        drop(ctx);

        if let Some(app_path) = app_path
            && !rows.is_empty()
        {
            super::open_with::launch_with(app, &rows, app_path, &listing_id);
        } else {
            log::warn!("Open with: missing app or paths for {bundle_id}");
        }
        return;
    }

    // === Open with → Other… : show NSOpenPanel, then launch ===
    #[cfg(target_os = "macos")]
    if id == super::open_with::OPEN_WITH_OTHER_ID {
        use crate::file_system::open_with::pick_app_via_open_panel;

        let menu_state = app.state::<MenuState<tauri::Wry>>();
        let ctx = menu_state.context.lock_ignore_poison();
        let rows = ctx.paths.clone();
        let listing_id = ctx.listing_id.clone();
        drop(ctx);

        // NSOpenPanel must run on the main thread. on_menu_event is invoked on
        // the main thread by Tauri/muda, so this is safe.
        if let Some(app_path) = pick_app_via_open_panel()
            && !rows.is_empty()
        {
            super::open_with::launch_with(app, &rows, app_path, &listing_id);
        }
        return;
    }

    // === Share → one service: run it on the rows the submenu was built from ===
    // The ID carries an index into the offer `show_file_context_menu` enumerated, so
    // this needs no path lookup at all: the service performs on the very `NSURL`s
    // macOS vetted, which is also what keeps it on the RIGHT-CLICKED rows rather than
    // the focused-pane selection.
    //
    // Deferred one main-thread turn instead of performed inline: we're inside muda's
    // action while the menu's own tracking loop is still unwinding, and a service
    // usually puts a window or sheet up, which that unwind can dismiss.
    #[cfg(target_os = "macos")]
    if let Some(index) = super::share_submenu::share_service_index(id) {
        use crate::file_system::share::{ShareError, perform_offered};

        if let Err(e) = app.run_on_main_thread(move || {
            let Some(mtm) = objc2::MainThreadMarker::new() else {
                log::warn!(target: "menu", "Share: the main-thread hop didn't land on the main thread");
                return;
            };
            if let Err(reason) = perform_offered(mtm, index) {
                // A refusal is a share that never happened, which the user sees as
                // nothing happening; naming the state is what makes that debuggable.
                let state = match reason {
                    ShareError::NoSuchService => "no service at that index",
                };
                log::warn!(target: "menu", "Share didn't start: {state}");
            }
        }) {
            log::warn!(target: "menu", "Share: couldn't reach the main thread: {e}");
        }
        return;
    }

    // === A File Provider action: the provider's own ===
    // The ID carries an index into the offer the menu was built from, so the action runs on
    // exactly the rows and domain that offer was evaluated for. Not through
    // `menu_id_to_command`: the items exist only once File Provider vouched for the rows,
    // which a palette entry or shortcut couldn't check first. The provider shows its own
    // windows, and the File Provider calls run on a thread of their own.
    #[cfg(target_os = "macos")]
    if let Some(index) = super::file_provider_items::file_provider_action_index(id) {
        let menu_state = app.state::<MenuState<tauri::Wry>>();
        let offer = menu_state.context.lock_ignore_poison().file_provider_offer.clone();
        match offer {
            Some(offer) => {
                if let Err(reason) = crate::file_system::file_provider_actions::perform(&offer, index) {
                    log::warn!(target: "menu", "File Provider action didn't start: {reason:?}");
                }
            }
            None => log::warn!(target: "menu", "File Provider action clicked with no offer armed"),
        }
        return;
    }

    // === Share → Edit extensions: System Settings' Extensions pane ===
    // Handled here rather than through `menu_id_to_command` because it isn't a file
    // command: there's nothing to bind a shortcut to and nothing for the palette to
    // offer, and the whole `Share` submenu is built and routed in the backend anyway.
    // `open_system_settings_url` is the house helper for these deep links (the Tauri
    // opener plugin's allowlist drops the `x-apple.systempreferences:` scheme silently).
    #[cfg(target_os = "macos")]
    if id == super::share_submenu::SHARE_EDIT_EXTENSIONS_ID {
        if let Err(e) =
            crate::permissions::open_system_settings_url(super::share_submenu::EXTENSIONS_SETTINGS_URL.to_string())
        {
            log::warn!(target: "menu", "Share: couldn't open the Extensions settings pane: {e}");
        }
        return;
    }

    // === Tag color items: prefix-routed straight to the tag write (like open-with) ===
    // `tag-color:<index>` toggles that system color on the RIGHT-CLICKED selection
    // (`MenuState.context.paths`), then refreshes the stashed listing's cache. It acts on
    // the right-clicked set, not the focused-pane selection, so it can't route through
    // `execute-command` + a frontend command (those read the focused selection — wrong
    // when the right-click landed on an unselected row). The keyboard `tags.toggle*`
    // commands handle the focused-selection case separately.
    #[cfg(target_os = "macos")]
    if let Some(rest) = id.strip_prefix(super::TAG_COLOR_ID_PREFIX) {
        if let Ok(color) = rest.parse::<u8>() {
            let menu_state = app.state::<MenuState<tauri::Wry>>();
            let ctx = menu_state.context.lock_ignore_poison();
            let paths = ctx.paths.clone();
            let listing_id = ctx.listing_id.clone();
            drop(ctx);
            if !paths.is_empty() {
                crate::file_system::tags_io::toggle_from_menu(app.clone(), listing_id, paths, color);
            }
        }
        return;
    }

    // === Unified dispatch: look up command ID from the mapping ===
    if let Some((command_id, scope)) = menu_id_to_command(id) {
        emit_command(app, command_id, scope, &source);
    }

    // Unknown menu ID: no-op (all known IDs are handled above)
}

/// Sends `command_id` to the main window down `source.road`. A file-scoped one runs only from the
/// main window: the page it came from where that's known, else the window with focus.
fn emit_command(app: &AppHandle<tauri::Wry>, command_id: &'static str, scope: CommandScope, source: &PickSource) {
    let road = source.road;
    if scope == CommandScope::FileScoped {
        // Focus guard: a file command acts on the main window's pane, so it must not
        // fire while the user is in Settings or the viewer — a native accelerator reaches
        // this handler whatever has focus, and a greyed-looking item is only a hint. A page's
        // pick names its own window, which is surer than focus: an X session with no window
        // manager (the Linux E2E lane) never reports the main window focused.
        let focused = source.is_from_main_window(app);
        if !focused {
            // ❗ Say so. This refuses a click the user just made and shows them
            // nothing, so with no line here the symptom is "the menu did nothing"
            // and the log holds no trace of it at all: the context menu had already
            // drawn the item as enabled, and the drop happens after that. Cost one
            // debug line rather than another blind bug report.
            log::debug!(
                target: "menu",
                "dropped `{command_id}`: file-scoped, and the main window doesn't have focus"
            );
            return;
        }
    }
    use tauri_specta::Event as _;
    let _ = crate::window_events::ExecuteCommand {
        command_id: command_id.to_string(),
        road,
    }
    .emit_to(app, "main");
}

/// Runs a context-menu row's own lane. Every one acts on what the popup was opened over, which the
/// `show_*` command stashed in `MenuState` (a path, a volume, a host row), or carries it in the id
/// (a tab action, a date-sort pane).
fn dispatch_context_click(app: &AppHandle<tauri::Wry>, click: ContextClick, source: &PickSource) {
    use tauri_specta::Event as _;
    let menu_state = app.state::<MenuState<tauri::Wry>>();
    let context_path = || menu_state.context.lock_ignore_poison().path.clone();
    match click {
        // Handled by the caller: it shares the bar's unified dispatch.
        ContextClick::Command { .. } | ContextClick::RowCommand { .. } => {}
        ContextClick::ShowSearchResultInFolder => {
            let path = search_result_reveal_path(&menu_state.context.lock_ignore_poison());
            if let Some(path) = path {
                let _ = crate::window_events::ShowSearchResultInFolder { path }.emit_to(app, "main");
            }
        }
        // Favorites the right-clicked path (the folder for a folder row, the parent dir for `..`),
        // never the focused pane's folder. ❗ Through the `add_favorite` COMMAND rather than
        // `favorites::store::add`, so this surface meets the same add gate as the palette and the
        // MCP tool; a `store::add` here would let a right-click inside an archive or on a phone
        // store a favorite nothing can ever show. The command also owns the blocking pool, the
        // timeout, and the `volumes-changed` re-emit.
        ContextClick::AddFavorite => {
            let path = context_path();
            if path.is_empty() {
                log::warn!(target: "favorites", "Add to favorites: empty context path, ignoring");
                return;
            }
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::commands::favorites::add_favorite(path, None).await {
                    log::warn!(target: "favorites", "Add to favorites: {e}");
                }
            });
        }
        // The image-search pair act on the RIGHT-CLICKED folder and drive a frontend persist path
        // (the native menu can't write the frontend store): the frontend writes
        // `mediaIndex.excludedFolders` / `alwaysIndexFolders` and calls the matching command.
        ContextClick::MediaIndexExclusion { excluded } => {
            let folder = context_path();
            if folder.is_empty() {
                log::warn!(target: "media_index", "folder exclusion clicked with no context path, ignoring");
                return;
            }
            let _ = MediaIndexFolderExclusion { folder, excluded }.emit_to(app, "main");
        }
        ContextClick::MediaIndexChoice { chosen } => {
            let folder = context_path();
            if folder.is_empty() {
                log::warn!(target: "media_index", "folder choice clicked with no context path, ignoring");
                return;
            }
            let _ = MediaIndexFolderChoice { folder, chosen }.emit_to(app, "main");
        }
        ContextClick::Tab(action) => {
            let _ = crate::window_events::TabContextAction {
                action: action.to_string(),
            }
            .emit_to(app, "main");
        }
        // No context to stash: the frontend owns both the setting write and the toast.
        ContextClick::HideFunctionKeyBar => {
            let _ = crate::window_events::FunctionKeyBarHideRequested.emit_to(app, "main");
        }
        // The breadcrumb's detach items. (A switcher row's and a favorite's actions never come
        // through here: they're the in-app `Menu`'s, picked in the frontend.)
        ContextClick::Volume(action) => {
            let ctx = menu_state.volume_row_context.lock_ignore_poison();
            if ctx.volume_id.is_empty() {
                log::warn!(target: "menu", "Volume row menu item {action:?} clicked with no volume_id stashed");
                return;
            }
            let _ = crate::volume_broadcast::VolumeContextAction {
                action: volume_action_kind(action),
                volume_id: ctx.volume_id.clone(),
                volume_name: ctx.volume_name.clone(),
            }
            .emit_to(app, "main");
        }
        ContextClick::NetworkHost(action) => {
            let ctx = menu_state.network_host_context.lock_ignore_poison();
            let _ = crate::network::NetworkHostContextAction {
                action: network_host_action_kind(action),
                row_id: ctx.row_id.clone(),
                host_id: ctx.host_id.clone(),
                host_name: ctx.host_name.clone(),
            }
            .emit_to(app, "main");
        }
        // `show_viewer_context_menu` focused the viewer before the popup, so the focused one is
        // the one it was popped over.
        ContextClick::ViewerText(action) => {
            let Some(label) = source.viewer_label(app) else {
                log::warn!(target: "menu", "Viewer context item {action:?} clicked with no viewer focused, ignoring");
                return;
            };
            let _ = crate::window_events::ViewerContextMenuAction { action }.emit_to(app, &label);
        }
        ContextClick::QueueRow(action) => {
            let event = crate::window_events::QueueRowContextAction { action };
            if let Err(e) = event.emit_to(app, "queue") {
                log::warn!(target: "menu", "failed to emit queue-row-context-action: {e}");
            }
        }
        ContextClick::DateSort { pane, column } => {
            if let Err(e) = (super::DateSortMenuPick { pane, column }).emit_to(app, "main") {
                log::warn!(target: "menu", "failed to emit date-sort-menu-pick: {e}");
            }
        }
    }
}

/// The frontend's volume action for a breadcrumb detach item. Exhaustive both ways, so a new
/// variant on either side is a compile error here.
fn volume_action_kind(action: VolumeAction) -> VolumeContextActionKind {
    match action {
        VolumeAction::Eject => VolumeContextActionKind::Eject,
        VolumeAction::EjectAll => VolumeContextActionKind::EjectAll,
    }
}

/// The frontend's host action for a servers hub host item.
fn network_host_action_kind(action: NetworkHostAction) -> crate::network::NetworkHostContextActionKind {
    use crate::network::NetworkHostContextActionKind as Kind;
    match action {
        NetworkHostAction::ForgetServer => Kind::ForgetServer,
        NetworkHostAction::ForgetSecret => Kind::ForgetSecret,
        NetworkHostAction::Disconnect => Kind::Disconnect,
        NetworkHostAction::Edit => Kind::Edit,
    }
}

#[cfg(test)]
mod context_click_wire_tests {
    use super::*;

    /// The demo reads these lanes' wire words off the crate (`VolumeAction::wire`), and the
    /// desktop emits its own enums: the two have to serialize alike.
    #[test]
    fn the_crate_s_wire_words_are_the_events_own() {
        for action in [VolumeAction::Eject, VolumeAction::EjectAll] {
            assert_eq!(
                serde_json::to_value(volume_action_kind(action)).expect("serializes"),
                action.wire()
            );
        }
        for action in [
            NetworkHostAction::ForgetServer,
            NetworkHostAction::ForgetSecret,
            NetworkHostAction::Disconnect,
            NetworkHostAction::Edit,
        ] {
            assert_eq!(
                serde_json::to_value(network_host_action_kind(action)).expect("serializes"),
                action.wire()
            );
        }
    }
}

/// The primary row path carried by the snapshot-only context action.
fn search_result_reveal_path(context: &super::MenuContext) -> Option<String> {
    (!context.path.is_empty()).then(|| context.path.clone())
}

#[cfg(test)]
mod viewer_history_direction_tests {
    use super::*;
    use crate::menu::{GO_BACK_ID, GO_FORWARD_ID};

    #[test]
    fn the_viewer_bars_go_items_name_their_direction() {
        assert_eq!(
            viewer_history_direction_for(VIEWER_GO_BACK_ID),
            Some(ViewerHistoryDirection::Back)
        );
        assert_eq!(
            viewer_history_direction_for(VIEWER_GO_FORWARD_ID),
            Some(ViewerHistoryDirection::Forward)
        );
    }

    /// ❗ The main bar's Back / Forward move a pane; one landing here would step a viewer's
    /// document history instead.
    #[test]
    fn the_main_bars_go_items_are_not_viewer_steps() {
        assert_eq!(viewer_history_direction_for(GO_BACK_ID), None);
        assert_eq!(viewer_history_direction_for(GO_FORWARD_ID), None);
        assert_eq!(viewer_history_direction_for(VIEWER_WORD_WRAP_ID), None);
    }
}

#[cfg(test)]
mod viewer_edit_action_tests {
    use super::*;
    use crate::menu::{EDIT_COPY_ID, SELECT_ALL_ID, VIEWER_CONTEXT_COPY_ID, VIEWER_CONTEXT_SELECT_ALL_ID};

    /// The viewer text menu's action, as the context-menu lane reads it.
    fn viewer_context_action_for(menu_id: &str) -> Option<ViewerEditActionKind> {
        match context_click(menu_id) {
            Some(ContextClick::ViewerText(action)) => Some(action),
            _ => None,
        }
    }

    #[test]
    fn the_viewer_bars_edit_items_name_their_action() {
        assert_eq!(
            viewer_edit_action_for(VIEWER_EDIT_COPY_ID),
            Some(ViewerEditActionKind::Copy)
        );
        assert_eq!(
            viewer_edit_action_for(VIEWER_SELECT_ALL_ID),
            Some(ViewerEditActionKind::SelectAll)
        );
    }

    /// ❗ Both bars share `EDIT_MENU_ID`, and the main bar's Copy / Select all route
    /// somewhere else entirely (the main window, or the native responder chain). A main-bar
    /// id falling into this branch would copy the wrong thing from the wrong window.
    #[test]
    fn a_main_bar_edit_id_is_not_a_viewer_edit_action() {
        assert_eq!(viewer_edit_action_for(EDIT_COPY_ID), None);
        assert_eq!(viewer_edit_action_for(SELECT_ALL_ID), None);
        assert_eq!(viewer_edit_action_for(VIEWER_WORD_WRAP_ID), None);
        assert_eq!(viewer_edit_action_for("unknown_id"), None);
    }

    #[test]
    fn the_viewer_context_menus_items_name_their_action() {
        assert_eq!(
            viewer_context_action_for(VIEWER_CONTEXT_COPY_ID),
            Some(ViewerEditActionKind::Copy)
        );
        assert_eq!(
            viewer_context_action_for(VIEWER_CONTEXT_SELECT_ALL_ID),
            Some(ViewerEditActionKind::SelectAll)
        );
    }

    /// ❗ The bar's pair defers to the search box when it has focus; the right-click pair
    /// always acts on the file. Crossing the two would copy the query from a menu opened over
    /// the text, or the reverse.
    #[test]
    fn the_bar_and_the_context_menu_never_share_an_item() {
        assert_eq!(viewer_context_action_for(VIEWER_EDIT_COPY_ID), None);
        assert_eq!(viewer_context_action_for(VIEWER_SELECT_ALL_ID), None);
        assert_eq!(viewer_edit_action_for(VIEWER_CONTEXT_COPY_ID), None);
        assert_eq!(viewer_edit_action_for(VIEWER_CONTEXT_SELECT_ALL_ID), None);
    }
}

#[cfg(test)]
mod native_edit_selector_tests {
    use super::*;

    /// The guarantee the viewer's Cut and Paste exist for: whatever else changes about them,
    /// the click still ends as `cut:` / `paste:` on the first responder, which is the search
    /// box. Trimming these two once left ⌘X / ⌘V dead in the viewer's search field.
    #[test]
    fn the_viewer_bars_cut_and_paste_forward_the_native_selector() {
        assert_eq!(
            native_edit_selector_for(VIEWER_EDIT_CUT_ID),
            Some(NativeEditSelector::Cut)
        );
        assert_eq!(
            native_edit_selector_for(VIEWER_EDIT_PASTE_ID),
            Some(NativeEditSelector::Paste)
        );
    }

    #[test]
    fn the_main_bars_four_edit_items_keep_their_selectors() {
        assert_eq!(native_edit_selector_for(EDIT_CUT_ID), Some(NativeEditSelector::Cut));
        assert_eq!(native_edit_selector_for(EDIT_COPY_ID), Some(NativeEditSelector::Copy));
        assert_eq!(native_edit_selector_for(EDIT_PASTE_ID), Some(NativeEditSelector::Paste));
        assert_eq!(
            native_edit_selector_for(SELECT_ALL_ID),
            Some(NativeEditSelector::SelectAll)
        );
    }

    /// The PDF view's Copy and Select all go native: its text lives in the embed, which only
    /// the responder chain reaches, and a Copy anywhere else would miss it.
    #[test]
    fn the_viewer_pages_pdf_copy_and_select_all_map_to_the_native_pair() {
        assert_eq!(
            native_edit_selector_for_viewer_page(ViewerEditActionKind::Copy),
            NativeEditSelector::Copy
        );
        assert_eq!(
            native_edit_selector_for_viewer_page(ViewerEditActionKind::SelectAll),
            NativeEditSelector::SelectAll
        );
    }

    /// ❗ The viewer's Copy and Select all act on its own offset-based selection, which lives
    /// outside the DOM the responder chain reaches. A native selector would land on the status
    /// bar instead, which is the bug `viewer_edit_action_for` exists to avoid.
    #[test]
    fn the_viewer_bars_copy_and_select_all_forward_nothing() {
        assert_eq!(native_edit_selector_for(VIEWER_EDIT_COPY_ID), None);
        assert_eq!(native_edit_selector_for(VIEWER_SELECT_ALL_ID), None);
        assert_eq!(native_edit_selector_for(VIEWER_WORD_WRAP_ID), None);
        assert_eq!(native_edit_selector_for("unknown_id"), None);
    }
}

#[cfg(test)]
mod search_result_reveal_tests {
    use super::*;
    use crate::menu::MenuContext;

    #[test]
    fn show_in_folder_carries_the_primary_right_clicked_path_not_the_selection() {
        let context = MenuContext {
            path: "/share/B.txt".to_string(),
            paths: vec!["/share/A.txt".to_string(), "/share/C.txt".to_string()],
            ..MenuContext::default()
        };

        assert_eq!(search_result_reveal_path(&context), Some("/share/B.txt".to_string()));
        assert_eq!(search_result_reveal_path(&MenuContext::default()), None);
    }
}
