# In-page right-click menus: details

The component, keys, placement, and dismissal are `#lib/ui/ContextMenu.svelte`'s (`../ui/DETAILS.md` § Command menus).
The backend half (`open_context_menu_in_page`, the token, the stash) is `src-tauri/src/menu/DETAILS.md` § "Right-click
menus a page draws". This module is the window's glue between them.

## The `show_*` menus

- **Where the fork lives**: each of the eight wrappers (`showFileContextMenu`, `showBreadcrumbContextMenu`,
  `showParentRowContextMenu`, `showTabContextMenu`, `showDateSortMenu`, `showFunctionKeyBarContextMenu`,
  `showViewerContextMenu`, `showQueueRowContextMenu`) calls `popContextMenu(request, native)`. On `native` it runs the
  `show_*` command; on `inPage` it asks `open_context_menu_in_page` with the same arguments and hands the answer to the
  window's drawer (`registerContextMenuDrawer`, which `InPageContextMenus` registers). No caller changed: each still
  names its menu's facts and, for a keyboard-opened menu, an anchor.
- Decision/Why: the wrappers, ❌ not the call sites. Nine call sites (and the search dialog and the search-results pane)
  would each grow the same branch, and one forgotten branch would pop a native menu on Windows.
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
- **A late answer draws nothing**: `popContextMenu` drops an answer once a later ask has gone out, and the host drops
  one asked before a menu of its own (a text field's) opened. Either would draw at the newer click's point a menu whose
  pick the backend calls `stale`.
- **What a scroll closes**: a scroll of anything holding the element the last right-click landed on (or the focused
  element for a key), so a pane scrolling under its menu closes it, as a native one does. The holders are read as the
  menu opens (`context-menu.svelte.ts`), so a virtual list re-rendering the row meanwhile still closes it.

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
- **Focus stays in the field** (`ContextMenuOpen.focusHolder`): the menu never takes it, and the keyboard's cursor
  reaches a screen reader as the field's `aria-activedescendant`. Every key while it's up goes to the menu, a key it
  doesn't use included, so nothing types behind it. A press on the menu (a row, a separator, its padding) is
  `preventDefault`ed on `pointerdown`, so the page gets no `mousedown` and focus never moves.
  - Gotcha/Why: when the menu took focus, every field's blur handler ran as it opened. The inline rename editor
    cancelled (`InlineRenameEditor`'s `onblur`), Ask Cmdr's session rename committed a half-typed title, and
    multi-rename's token editors closed, so Paste found a detached field. A rule for every field to ignore a menu's blur
    would be a rule forever; keeping focus is what the engine's own menu does. Pinned by
    `InPageContextMenus.svelte.test.ts` with the real rename editor.
  - A field inside the house `Menu` (the volume switcher's favorite rename): `Menu.svelte`'s outside-press check counts
    `[data-command-menu="context"]` as its own, so pressing a row doesn't close it and unmount the field.
- **Edits** (`runTextFieldEdit`) act on the field the menu opened on, and only while it still holds focus, ❌ never on
  whatever has focus by then. `execCommand` for Undo, Redo, Cut, and Copy; Select all is `select()` or
  `selectAllChildren`. Paste reads the clipboard through the backend (`readClipboardText`, no engine permission prompt),
  then offers it to the field's own paste handling as a real paste does, a `paste` event carrying the text: a field that
  refuses it (`RangeTokenEditor`) or inserts it itself `preventDefault`s, and the menu inserts nothing. Otherwise it
  inserts with `insertText`, which keeps the field's undo step; the field's `input` event then reads
  `inputType: 'insertText'`, ❌ not `'insertFromPaste'` (no field in the app reads that).
- **Selection and caret** stay the field's: saved as the menu opens and put back before an edit, after Escape, and after
  Tab. Right-click's own caret rule (inside the selection keeps it, outside moves the caret) is the engine's, before the
  menu opens.
- **An input method**: no menu opens while a composition is running (the right-click is the input method's), and a
  composition starting closes one.
- **Shift+F10 and the Menu key in a field**: `native-text-keys.ts` leaves both to the field, so the engine raises a
  `contextmenu` on it, which opens this menu at the point the engine named inside the field, else under it.
- Decision/Why: the rows live here, not in `cmdr-menu`. They aren't Cmdr commands: they stand in for the engine's own
  edit menu, and each acts on a field only the page can reach, so a pick never goes to the backend. (Not user
  activation: Chromium's lasts about five seconds after the gesture, which a backend round trip wouldn't spend.) The
  cost is that the labels and keys live in TS beside `cmdr-menu`'s Edit menu data; moving the rows into `cmdr-menu` with
  the edit kept page-side is open, and small.

## Keys

- `file.contextMenu` binds Shift+F10, the Menu key (`ContextMenu`, shown as "Menu"), and `Ctrl+Enter` off macOS. The
  Menu key's keydown, claimed by the dispatcher, opens the cursor row's menu at the row. Left alone, the engine raises a
  `contextmenu` on the focused list, which no row hears.

## Checked on WebView2

Verified on Windows 10 22H2 build 19045.6466, WebView2 157.0.4341.60, with real OS input (`keybd_event`) and
screenshots, 2026-11-19 (`docs/notes/windows-port/real-windows-check-2026-11-19.md`):

- A keyboard-raised menu in a field: Shift+F10 and the Menu key in the rename editor open the edit menu once, just under
  the field's start, with its first live row highlighted. Not read directly: whether `isKeyboardContextMenu` saw an
  empty `pointerType` (no DOM access on that run), only that the menu behaved as keyboard-opened.
- The Menu key on a pane row opens exactly one in-page menu at the row, and no WebView2 menu follows, so either no
  second `contextmenu` comes or the ownership handler eats it; which one is unread.
- `execCommand('cut')` / `'copy'` from a pick work: Copy and Cut put the field's text on the system clipboard (read back
  with `Clipboard.GetText`), and Cut emptied the field.
- `queryCommandEnabled('undo')` reads the field's own stack: Undo and Redo are greyed on a fresh rename field, and Undo
  turns live after a Paste (and after typing in Settings' search), then restores the text.

## Running them off Windows

- **By hand**: `CMDR_MOCK_CONVENTIONS=windows pnpm dev` draws every in-page menu on a Mac (a debug build only; it moves
  every convention and the copy too).
- **E2E**: `test/e2e-playwright/in-page-context-menus.spec.ts` (`@in-page-menus`) drives them end to end: a right-click
  and a pick through the backend, Shift+F10 and the Menu key, Escape with focus back on the list, and the rename
  editor's edit menu. Its tests skip on an app whose `contextMenus` isn't `inPage`. The Linux lane relaunches the app
  with `CMDR_E2E_CONTEXT_MENUS=inPage` after the suite and runs that group (`scripts/e2e-linux.sh`, shard 1 only).
- The override moves `contextMenus` alone, and only a `playwright-e2e` build reads it
  (`src-tauri/src/platform/frontend.rs`). Release builds compile default features, which don't hold that feature, so a
  shipped app ignores the variable; `the_e2e_context_menus_override_is_absent_here` pins that.
- That lane is WebKitGTK, not WebView2; the WebView2 behavior above came from a run on mami, which no lane repeats yet
  (task 55).
