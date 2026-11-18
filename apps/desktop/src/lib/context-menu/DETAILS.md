# In-page right-click menus: details

The component, keys, placement, and dismissal are `#lib/ui/ContextMenu.svelte`'s (`../ui/DETAILS.md` § Command menus).
The backend half (`open_context_menu_in_page`, the token, the stash) is `src-tauri/src/menu/DETAILS.md` § "Right-click
menus a page draws". This module is the window's glue between them.

## The `show_*` menus

- **Where the fork lives**: each of the nine wrappers (`showFileContextMenu`, `showBreadcrumbContextMenu`,
  `showParentRowContextMenu`, `showTabContextMenu`, `showDateSortMenu`, `showFunctionKeyBarContextMenu`,
  `showViewerContextMenu`, `showQueueRowContextMenu`, `showNetworkHostContextMenu`) calls
  `popContextMenu(request, native)`. On `native` it runs the `show_*` command; on `inPage` it asks
  `open_context_menu_in_page` with the same arguments and hands the answer to the window's drawer
  (`registerContextMenuDrawer`, which `InPageContextMenus` registers). No caller changed: each still names its menu's
  facts and, for a keyboard-opened menu, an anchor.
- Decision/Why: the wrappers, ❌ not the call sites. Nine call sites (and the search dialog, the search-results pane,
  the servers hub) would each grow the same branch, and one forgotten branch would pop a native menu on Windows.
- **Where it opens**: the answer's `anchor` when the caller named one (Shift+F10 or the Menu key on a file row,
  `context-menu-anchor.ts`; the date chevron), else the last pointer right-click's point, which the host follows with a
  capture `contextmenu` listener. A menu raised by a key with no anchor opens under the focused element.
- **From the keyboard or not**: whichever came last of a key (modifiers alone don't count) or a pointer press. A
  keyboard-opened menu starts its cursor on the first live row and shows its underlines from the start; a clicked one
  doesn't. So the date chevron clicked opens with no cursor, and the same chevron reached with Enter opens on its first
  row.
- **Settling**: each open is a session. A pick closes the menu, puts focus back, then awaits `pickContextMenuItem` and
  settles; a dismissal settles at once; a menu opening over another settles the old one first. The wrapper's promise
  resolves on settle, so `tab-operations.ts`'s "closed with no pick" timeout and the date menu's resolve-on-close keep
  their native meaning.
- **What a scroll closes**: the element the last right-click landed on (or the focused element for a key), so a pane
  scrolling under its menu closes it, as a native one does.

## The text-field menu

David's decision (2026-11-18): on Windows a text field's right-click opens Cmdr's in-page edit menu, so every menu in
the app looks the same. `#lib/context-menu-ownership.ts` decides per right-click; where menus are native it keeps the
engine's own edit menu.

- **Which fields** (`textTargetOf`): `<textarea>`, `<input>` of type text, search, url, tel, email, password, or number,
  and `contenteditable` (the outermost editable host). A disabled field, a checkbox, a slider, a `<select>`, and other
  non-text inputs get no menu at all, and the event still stops before a row's handler opens its menu: Win32 shows none
  on a disabled edit control.
- **Rows** (`textFieldMenuRows`): Undo, Redo, Cut, Copy, Paste, Select all, with the Edit menu's catalog keys and the
  engine's own keys as text (`Ctrl+Y` for Redo on Windows, `Ctrl+Shift+Z` elsewhere, the ⌘ forms on a Mac dev run).
  Greyed by the field's facts as it opens: read-only greys Undo, Redo, Cut, and Paste; no selection greys Cut and Copy;
  a password greys Cut and Copy always, as the engine's own menu does; an empty field greys Select all. Undo and Redo
  ask `queryCommandEnabled`, true when the engine can't say. `number` and `email` inputs keep no selection range, so
  they read as "maybe selected" and keep Copy live.
- **Edits** (`runTextFieldEdit`): `execCommand` for Undo, Redo, Cut, and Copy; Paste reads the clipboard through the
  backend (`readClipboardText`, no engine permission prompt) and inserts with `insertText`, which keeps the field's undo
  step; Select all is `select()` or `selectAllChildren`.
- **Selection and caret**: the menu takes focus, so the field's selection is saved as it opens and put back before an
  edit runs, and after a dismissal that returns focus (Escape, Tab). A click elsewhere leaves focus where it went.
  Right-click's own caret rule (inside the selection keeps it, outside moves the caret) is the engine's, before the menu
  opens.
- **Shift+F10 and the Menu key in a field**: `native-text-keys.ts` leaves both to the field, so the engine raises a
  `contextmenu` on it, which opens this menu at the point the engine named inside the field, else under it.
- Decision/Why: the rows live here, ❌ not in `cmdr-menu`. They aren't Cmdr commands: they stand in for the engine's
  edit menu, and each acts on a field only the page can reach. Through the backend's pick, the edit would run on an
  event after the round trip, and Chromium's `execCommand('copy')` and `'cut'` need the user activation the pick's own
  click or key carries.

## Keys

- `file.contextMenu` binds Shift+F10, the Menu key (`ContextMenu`, shown as "Menu"), and `Ctrl+Enter` off macOS. The
  Menu key's keydown, claimed by the dispatcher, opens the cursor row's menu at the row. Left alone, the engine raises a
  `contextmenu` on the focused list, which no row hears.

## Unverified on Windows

- How WebView2 marks a keyboard-raised `contextmenu` (`isKeyboardContextMenu`: an empty `pointerType`, Chromium's
  documented behavior) and where it places one inside a field.
- Whether WebView2 still raises a `contextmenu` after the Menu key's keydown is `preventDefault`ed. If it does, it lands
  on the focused list (or the open menu) and the ownership handler suppresses it.
- `execCommand('cut')` / `'copy'` under WebView2 from a pick: Chromium allows both with user activation.

E2E: no lane draws in-page menus today (macOS and Linux are `native`, and the conventions global can't be flipped in a
running app), so the coverage is `InPageContextMenus.svelte.test.ts` until a Windows lane runs (task 55).
