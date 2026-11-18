//! Pins the bars a page draws (`page::bar`, `page::viewer`) and the change diff.

use std::collections::HashSet;

use super::viewer::{ViewerBarInputs, viewer_bar_view};
use super::{BarInputs, BarMenuView, Row, RowLabel, RowState, bar_view, changed_rows};
use crate::bar::menus_on;
use crate::spec::{Pane, Platform};

/// `kind id label [shown|aria] state`, nested rows indented under their submenu.
const WINDOWS_PAGE_BAR: &str = "\
menu menu_file menu.bar.file
  item open menu.file.open
  item file_view menu.file.view [F3|F3]
  item edit menu.file.edit [F4|F4]
  separator
  item file_copy menu.file.copy [F5|F5]
  item file_move menu.file.move [F6|F6]
  item file_duplicate menu.file.duplicate [Shift+F5|Shift+F5]
  item file_compress menu.file.compress [Alt+F5|Alt+F5]
  item file_new_folder menu.file.newFolder [F7|F7]
  item file_new_file menu.file.newFile [Shift+F4|Shift+F4]
  item file_delete menu.file.delete [F8|F8]
  item file_delete_permanently menu.file.deletePermanently [Shift+F8|Shift+F8]
  separator
  item rename menu.file.rename [F2|F2]
  item multi_rename menu.file.multiRename [Ctrl+M|Control+M]
  separator
  item show_in_finder menu.file.showInFileManager [Ctrl+Shift+E|Control+Shift+E]
  item tell_me_about menu.file.tellMeAbout [Ctrl+Shift+Enter|Control+Shift+Enter]
  separator
  item quit menu.file.quit
menu menu_edit menu.bar.edit
  item undo_trash menu.edit.undoTrash disabled
  separator
  item edit_cut menu.edit.cut [Ctrl+X|Control+X]
  item edit_copy menu.edit.copy [Ctrl+C|Control+C]
  item edit_paste menu.edit.paste [Ctrl+V|Control+V]
  item edit_paste_move menu.edit.moveHere [Ctrl+Shift+V|Control+Shift+V]
  separator
  item copy_path menu.edit.copyPath [Ctrl+Shift+C|Control+Shift+C]
  item copy_filename menu.edit.copyFilename
  separator
  item search_files menu.edit.searchFiles [Ctrl+F|Control+F]
  separator
  item settings menu.app.settings [Ctrl+,|Control+,]
  item enter_license_key menu.app.licenseEnter
  item check_for_updates menu.app.checkForUpdates
  item changelog menu.app.changelog
menu menu_select menu.bar.select
  item select_all_files menu.select.all [Ctrl+A|Control+A]
  item deselect_all menu.select.deselectAll [Ctrl+Shift+A|Control+Shift+A]
  item select_same_kind menu.select.sameKind [Alt+Shift+=|Alt+Shift+=]
  item invert_selection menu.select.invert [*|*]
  separator
  item select_files menu.select.files [+|Plus]
  item deselect_files menu.select.deselectFiles [-|-]
  separator
  item compare_directories menu.select.compareDirectories [Shift+F2|Shift+F2]
menu menu_view menu.bar.view
  submenu menu.view.leftPane menu.view.leftPane
    radio view_mode_full_left menu.view.fullView [Ctrl+1|Control+1]
    radio view_mode_brief_left menu.view.briefView [Ctrl+2|Control+2] checked
  submenu menu.view.rightPane menu.view.rightPane
    radio view_mode_full_right menu.view.fullView
    radio view_mode_brief_right menu.view.briefView checked
  separator
  checkbox show_hidden_files menu.view.showHiddenFiles [Ctrl+Shift+.|Control+Shift+.]
  submenu menu_sort_by menu.view.sortBy
    radio sort_by_name menu.sort.name [Ctrl+3|Control+3]
    radio sort_by_extension menu.sort.extension [Ctrl+4|Control+4]
    radio sort_by_modified menu.sort.dateModified [Ctrl+5|Control+5]
    radio sort_by_size menu.sort.size [Ctrl+6|Control+6]
    radio sort_by_created menu.sort.dateCreated
    radio sort_by_added menu.sort.dateAdded
    separator
    radio sort_ascending menu.sort.ascending
    radio sort_descending menu.sort.descending
  submenu menu.view.zoom menu.view.zoom
    item view_zoom_75 menu.zoom.percent75
    item view_zoom_100 menu.zoom.percent100 [Ctrl+0|Control+0]
    item view_zoom_125 menu.zoom.percent125
    item view_zoom_150 menu.zoom.percent150
    separator
    item view_zoom_in menu.zoom.in [Ctrl++|Control+Plus]
    item view_zoom_out menu.zoom.out [Ctrl+-|Control+-]
  item view_full_screen menu.view.fullScreen [F11|F11]
  separator
  item switch_pane menu.view.switchPane [Tab|Tab]
  item swap_panes menu.view.swapPanes [Ctrl+U|Control+U]
  item clone_pane menu.view.clonePane [Ctrl+Shift+M|Control+Shift+M]
  item calculate_folder_sizes menu.view.calculateFolderSizes [Alt+Shift+Enter|Alt+Shift+Enter]
  separator
  item command_palette menu.view.commandPalette [Ctrl+Shift+P|Control+Shift+P]
  item queue_show menu.view.operationQueue [Ctrl+Shift+Q|Control+Shift+Q]
  item operation_log menu.view.operationLog [Ctrl+Shift+L|Control+Shift+L]
  item suggested_ops menu.view.suggestedOps
  item ask_cmdr menu.view.askCmdr [Ctrl+Shift+K|Control+Shift+K]
menu menu_go menu.bar.go
  item go_back menu.go.back [Alt+←|Alt+ArrowLeft]
  item go_forward menu.go.forward [Alt+→|Alt+ArrowRight]
  separator
  item go_parent menu.go.parentFolder [Alt+↑|Alt+ArrowUp]
  item go_root menu.go.rootFolder [Ctrl+\\|Control+\\]
  item go_home menu.go.home [Ctrl+Shift+H|Control+Shift+H]
  separator
  item go_to_path menu.go.goToPath [Ctrl+G|Control+G]
  item go_latest_download menu.go.goToLatestDownload [Ctrl+J|Control+J]
  separator
  item favorites_add menu.go.addToFavorites
  item favorites_open menu.go.showFavorites [Ctrl+D|Control+D]
  separator
  item servers_connect menu.go.connectToServer [Ctrl+K|Control+K]
  item servers_show menu.go.showServers
menu menu_tab menu.bar.tab
  item new_tab menu.tab.newTab [Ctrl+T|Control+T]
  item close_tab menu.tab.closeTab [Ctrl+W|Control+W]
  item reopen_closed_tab menu.tab.reopenClosedTab [Ctrl+Shift+T|Control+Shift+T] disabled
  separator
  item next_tab menu.tab.nextTab [Ctrl+Tab|Control+Tab]
  item prev_tab menu.tab.previousTab [Ctrl+Shift+Tab|Control+Shift+Tab]
  separator
  item pin_tab_menu menu.tab.pinTab
  item close_other_tabs menu.tab.closeOtherTabs
menu menu_help menu.bar.help
  item about menu.app.about
  item acknowledgements menu.app.acknowledgements
  separator
  item help_shortcuts menu.help.keyboardShortcuts
  item help_accessibility menu.help.accessibility
  item help_whats_new menu.help.whatsNew
  item help_send_feedback menu.help.sendFeedback
  item help_debug_log menu.help.viewDebugLog
  item help_send_error_report menu.help.sendErrorReport
";

const WINDOWS_VIEWER_PAGE_BAR: &str = "\
menu menu_file menu.bar.file
  item viewer_close menu.viewer.close [Ctrl+W|Control+W]
menu menu_edit menu.bar.edit
  item viewer_edit_cut menu.edit.cut [Ctrl+X|Control+X] disabled
  item viewer_edit_copy menu.edit.copy [Ctrl+C|Control+C]
  item viewer_edit_paste menu.edit.paste [Ctrl+V|Control+V] disabled
  separator
  item viewer_select_all menu.select.all [Ctrl+A|Control+A]
menu menu_view menu.bar.view
  checkbox viewer_word_wrap menu.viewer.wordWrap
menu menu_go menu.bar.go
  item viewer_go_back menu.go.back disabled
  item viewer_go_forward menu.go.forward disabled
";

fn render(menus: &[BarMenuView]) -> String {
    let mut out = String::new();
    for menu in menus {
        out.push_str(&format!("menu {} {}\n", menu.id, label(&menu.title)));
        render_rows(&menu.rows, "  ", &mut out);
    }
    out
}

fn render_rows(rows: &[Row], indent: &str, out: &mut String) {
    for row in rows {
        let line = match row {
            Row::Item {
                id,
                label: text,
                shortcut,
                aria_keyshortcuts,
                enabled,
            } => format!(
                "item {id} {}{}{}",
                label(text),
                keys(shortcut.as_deref(), aria_keyshortcuts.as_deref()),
                if *enabled { "" } else { " disabled" }
            ),
            Row::Check {
                id,
                label: text,
                shortcut,
                aria_keyshortcuts,
                enabled,
                checked,
                check_kind,
            } => format!(
                "{} {id} {}{}{}{}",
                format!("{check_kind:?}").to_lowercase(),
                label(text),
                keys(shortcut.as_deref(), aria_keyshortcuts.as_deref()),
                if *checked { " checked" } else { "" },
                if *enabled { "" } else { " disabled" }
            ),
            Row::Submenu {
                id,
                label: text,
                enabled,
                ..
            } => format!(
                "submenu {id} {}{}",
                label(text),
                if *enabled { "" } else { " disabled" }
            ),
            Row::Header { label: text } => format!("header {}", label(text)),
            Row::System { label: text } => format!("system {}", label(text)),
            Row::Separator => "separator".to_string(),
        };
        out.push_str(&format!("{indent}{line}\n"));
        if let Row::Submenu { rows, .. } = row {
            render_rows(rows, &format!("{indent}  "), out);
        }
    }
}

fn label(label: &RowLabel) -> String {
    match label {
        RowLabel::Key { key, args } if args.is_empty() => key.clone(),
        RowLabel::Key { key, args } => {
            let mut args: Vec<String> = args.iter().map(|(token, value)| format!("{token}={value}")).collect();
            args.sort();
            format!("{key}({})", args.join(","))
        }
        RowLabel::Text { text } => format!("\"{text}\""),
    }
}

fn keys(shown: Option<&str>, aria: Option<&str>) -> String {
    match (shown, aria) {
        (None, None) => String::new(),
        (shown, aria) => format!(" [{}|{}]", shown.unwrap_or("-"), aria.unwrap_or("-")),
    }
}

fn assert_pinned(what: &str, built: &str, pinned: &str) {
    if built == pinned {
        return;
    }
    let line = built
        .lines()
        .zip(pinned.lines())
        .position(|(built, pinned)| built != pinned)
        .unwrap_or_else(|| built.lines().count().min(pinned.lines().count()));
    panic!(
        "the {what} no longer matches the pinned one, from line {}:\n  pinned: {}\n  built:  {}\n\
         If the change is intended, update the pinned text. The whole bar as built:\n{built}",
        line + 1,
        pinned.lines().nth(line).unwrap_or("(nothing)"),
        built.lines().nth(line).unwrap_or("(nothing)"),
    );
}

#[test]
fn the_windows_page_bar() {
    let built = render(&bar_view(&BarInputs::fresh(Platform::Windows), Platform::Windows));
    assert_pinned("Windows page bar", &built, WINDOWS_PAGE_BAR);
}

#[test]
fn the_windows_viewer_page_bar() {
    let built = render(&viewer_bar_view(ViewerBarInputs::default(), Platform::Windows));
    assert_pinned("Windows viewer page bar", &built, WINDOWS_VIEWER_PAGE_BAR);
}

/// A change event names rows by id, so no two rows of one bar may share one.
#[test]
fn every_row_of_a_page_bar_has_its_own_id() {
    fn ids<'a>(rows: &'a [Row], out: &mut Vec<&'a str>) {
        for row in rows {
            out.extend(row.id());
            if let Row::Submenu { rows, .. } = row {
                ids(rows, out);
            }
        }
    }
    for platform in Platform::ALL {
        for menus in [
            bar_view(&BarInputs::fresh(platform), platform),
            viewer_bar_view(ViewerBarInputs::default(), platform),
        ] {
            let mut all = Vec::new();
            for menu in &menus {
                assert!(!menu.id.is_empty(), "a {platform:?} menu has no id");
                ids(&menu.rows, &mut all);
            }
            let mut seen = HashSet::new();
            for id in all {
                assert!(!id.is_empty(), "a {platform:?} row has an empty id");
                assert!(seen.insert(id), "`{id}` is on the {platform:?} page bar twice");
            }
        }
    }
}

/// The page bar holds exactly the native bar's menus, in order, on every platform.
#[test]
fn the_page_bar_has_the_native_bars_menus() {
    for platform in Platform::ALL {
        let page: Vec<String> = bar_view(&BarInputs::fresh(platform), platform)
            .into_iter()
            .map(|menu| menu.id)
            .collect();
        let native: Vec<String> = menus_on(platform)
            .map(|menu| menu.submenu.id.macos.unwrap_or_default().to_string())
            .collect();
        assert_eq!(page, native, "{platform:?}");
    }
}

fn state<'a>(changes: &'a [RowState], id: &str) -> &'a RowState {
    changes
        .iter()
        .find(|change| change.id == id)
        .unwrap_or_else(|| panic!("no change for `{id}`"))
}

/// A push moves only the rows it decides, and the diff names exactly those.
#[test]
fn a_change_names_only_the_rows_that_moved() {
    let platform = Platform::Windows;
    let mut inputs = BarInputs::fresh(platform);
    let before = bar_view(&inputs, platform);

    inputs.refused = HashSet::from(["file.copy".to_string()]);
    inputs.show_hidden_files = true;
    inputs.is_pinned = true;
    let after = bar_view(&inputs, platform);

    let changes = changed_rows(&before, &after).expect("same shape");
    let mut ids: Vec<&str> = changes.iter().map(|change| change.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["file_copy", "pin_tab_menu", "show_hidden_files"]);
    assert!(!state(&changes, "file_copy").enabled);
    assert_eq!(state(&changes, "show_hidden_files").checked, Some(true));
    assert_eq!(
        state(&changes, "pin_tab_menu").label,
        RowLabel::key("menu.tab.unpinTab")
    );

    assert_eq!(changed_rows(&after, &after), Some(Vec::new()));
}

/// The view-mode pair's key moves with the active pane, so the diff carries both pairs.
#[test]
fn the_view_mode_key_follows_the_active_pane() {
    let platform = Platform::Linux;
    let mut inputs = BarInputs::fresh(platform);
    let before = bar_view(&inputs, platform);
    inputs.active_pane = Pane::Right;
    let changes = changed_rows(&before, &bar_view(&inputs, platform)).expect("same shape");
    assert_eq!(state(&changes, "view_mode_full_left").shortcut, None);
    assert_eq!(
        state(&changes, "view_mode_full_right").shortcut.as_deref(),
        Some("Ctrl+1")
    );
    assert_eq!(
        state(&changes, "view_mode_full_right").aria_keyshortcuts.as_deref(),
        Some("Control+1")
    );
}

fn item_keys(menus: &[BarMenuView], id: &str) -> (Option<String>, Option<String>) {
    fn find(rows: &[Row], id: &str) -> Option<(Option<String>, Option<String>)> {
        rows.iter().find_map(|row| match row {
            Row::Item {
                id: own,
                shortcut,
                aria_keyshortcuts,
                ..
            } if own == id => Some((shortcut.clone(), aria_keyshortcuts.clone())),
            Row::Submenu { rows, .. } => find(rows, id),
            _ => None,
        })
    }
    menus
        .iter()
        .find_map(|menu| find(&menu.rows, id))
        .unwrap_or_else(|| panic!("no item `{id}`"))
}

/// A rebind draws through the bar's own door: registered past the modifier floor, else shown.
#[test]
fn a_rebind_shows_what_the_native_bar_would() {
    let platform = Platform::Windows;
    let mut inputs = BarInputs::fresh(platform);
    inputs.rebind("tab.new", "Ctrl+Alt+T", platform);
    inputs.rebind("tab.close", "Shift+8", platform);
    inputs.rebind("file.copy", "", platform);
    let bar = bar_view(&inputs, platform);
    assert_eq!(
        item_keys(&bar, "new_tab"),
        (Some("Ctrl+Alt+T".to_string()), Some("Control+Alt+T".to_string()))
    );
    assert_eq!(
        item_keys(&bar, "close_tab"),
        (Some("Shift+8".to_string()), Some("Shift+8".to_string()))
    );
    assert_eq!(item_keys(&bar, "file_copy"), (None, None));
}

/// A shape change (another platform's bar here) is no row diff: only a full answer describes it.
#[test]
fn a_shape_change_is_no_row_diff() {
    let windows = bar_view(&BarInputs::fresh(Platform::Windows), Platform::Windows);
    let macos = bar_view(&BarInputs::fresh(Platform::MacOs), Platform::MacOs);
    assert_eq!(changed_rows(&windows, &macos), None);
}

#[test]
fn the_viewer_bar_follows_the_viewers_state() {
    let platform = Platform::Windows;
    let off = viewer_bar_view(ViewerBarInputs::default(), platform);
    let on = viewer_bar_view(
        ViewerBarInputs {
            word_wrap: true,
            can_go_back: true,
            can_go_forward: false,
            search_focused: true,
        },
        platform,
    );
    let changes = changed_rows(&off, &on).expect("same shape");
    let mut ids: Vec<&str> = changes.iter().map(|change| change.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "viewer_edit_cut",
            "viewer_edit_paste",
            "viewer_go_back",
            "viewer_word_wrap"
        ]
    );
}
