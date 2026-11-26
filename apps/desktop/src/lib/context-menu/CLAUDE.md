# In-page right-click menus

Where `convention('contextMenus')` is `inPage` (Windows, D5 in `docs/specs/windows-port-plan.md`), every right-click
menu is drawn in the page by `#lib/ui/ContextMenu.svelte`: the eight `show_*` menus and the text fields' edit menu.
macOS and Linux keep native popups.

## Module map

- `InPageContextMenus.svelte`: the window's one menu, mounted by the root layout only where menus are in-page.
- `context-menu-host.svelte.ts`: opens a menu at the caller's anchor or the last right-click's point, and settles each
  one (its pick ran, or it closed).
- `window-context-menu.ts`: the window's host, and `openTextFieldMenu` for `#lib/context-menu-ownership.ts`.
- `text-field-menu.ts`: which fields get a menu, their facts, the rows, and the edits.

## Must-knows

- **The fork is in the `show_*` wrappers** (`popContextMenu` in `#lib/tauri-commands/in-page-menus.ts`), ❌ never at a
  call site: callers stay platform-blind, and a pick runs the native pick's handler.
- **A wrapper resolves once the menu is gone and its pick ran**, as a native `popup()` returns: the tab menu's dismissal
  timeout relies on it.
- **The text-field menu never takes focus** (`focusHolder`): the field keeps it, so its blur handlers never run (an
  inline rename would cancel). ❌ Never let it move focus. DETAILS § The text-field menu.
- **An edit acts on the field the menu opened on**, only while it holds focus; Paste goes through the field's own
  `paste` handling first, so a field that refuses a paste still does.
- **The text-field rows are the page's, not `cmdr-menu`'s**: their edit acts on a field only the page can reach.
- **Disabled fields, checkboxes, sliders, and `<select>` get no menu** where menus are in-page, as on Windows.

Mechanics, decisions, and what a WebView2 run checked: `DETAILS.md`.
