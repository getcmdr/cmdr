# First real-Windows check of tasks 34, 60, 62, 63, and 67 (2026-11-19)

The first run of the in-page menus, the browser-key switch, and the Windows look on a real Windows desktop. Everything
before this was built on a Mac with a mocked Windows look and checked on the Linux E2E lane (WebKitGTK).

- **Machine**: mami (`ssh mami`), Windows 10 Pro 22H2 build 19045.6466, Hungarian locale, en-US keyboard layout
  (`0x04090409`), light app mode, transparency effects on, accent `#0078D7`, 1600 × 900 at 100 %, WebView2 Evergreen
  157.0.4341.60. No Segoe UI Variable installed (`C:\Windows\Fonts\SegUIVar.ttf` absent).
- **Build**: `main` at `4314b3cf1f`, `pnpm tauri build --debug --no-bundle` in `D:\src\cmdr-verify`
  (`CARGO_TARGET_DIR=D:\targets\verify`, seeded from `D:\targets\triage`, 17m 58s), so the MCP bridge was in and
  `devtools` would have been reachable if F12 worked. The fixes below were re-run on `968733ae7f`.
- **How it ran**: a PowerShell driver in Admin's console session (started with
  `~/projects-git/vdavid/infra/mami-laptop/run-on-desktop.sh`) launched the app with its own `CMDR_DATA_DIR`, read
  queued command files, sent real input with `keybd_event` and `mouse_event` (so WebView2 saw OS keys, a lone Alt and
  AltGr's Ctrl+right Alt included), read `GetGUIThreadInfo` for Windows' menu mode (`GUI_INMENUMODE` 0x4,
  `GUI_SYSTEMMENUMODE` 0x8), read and set the clipboard, and saved window-cropped screenshots. The Tauri MCP bridge
  connected over `ssh -L` but every call failed ("WebView execution failed"), so DOM state came from screenshots and
  behavior only. Another agent's Windows Firewall prompt for `adb.exe` sat topmost over the desktop; it was moved aside,
  never answered.
- **Screenshots**: not in the repo; numbered as below in the session scratchpad's `win-verify/` folder
  (`/private/tmp/claude-501/…/scratchpad/win-verify/`), which a reboot clears.
- **A slip**: one queued step ran after Cmdr had quit, so Ctrl+Shift+Q and Ctrl+W reached another agent's `menu-spike`
  window (still running after). The driver now refuses input unless Cmdr is in front.

## Results, one line per check

Task 63 steps 6 and 7 (in-page menus):

- Pass: the bar shows in the main window (`03`) and the viewer (`90`), no native bar.
- Fail, fixed: a lone Alt tap gives the bar the keyboard (↓ then opens File, `11`) and shows the underlines, but drew no
  ring on File (`10`): Chromium doesn't count a modifier-only keydown as keyboard use, so `:focus-visible` missed. F10
  drew it (`20`), as did Escape back to a title (`13`). Re-run on `968733ae7f`: File draws its ring after a lone Alt
  (`202`).
- Pass: Alt, F10, and Alt plus a letter no title takes (Alt+Q) never start Windows' menu loop: `GetGUIThreadInfo` flags
  0 after each, and the next key reaches the page (`103`, `104`). No beep is expected with `SC_KEYMENU` swallowed; the
  box has no speaker check, so audibly untested.
- Pass: Alt+V opens View (`22`), Alt+E opens Edit; ↓ walks rows, → crosses to the next menu (`12`), Enter on a submenu
  row opens it (`24`).
- Pass from a focused pane: Escape closes the menu (focus on its title, `13`), the next Escape puts focus back on the
  pane list (↓ moved the cursor, `19`, `43`).
- Fail, fixed: with focus adrift on `<body>` (the first-launch "What's new" closed by a click), Alt then Escape twice
  left focus on the Edit title of a bar that had let go, and ↓ did nothing (`14`, `15`). Re-run on `968733ae7f` after
  the same click-closed dialog: Alt then Escape hands the pane the keys (`202`–`205`).
- Pass: picking View > Show hidden files lists the dot folders and the row shows its checkmark the next time (`26`,
  `27`); the viewer's View > Word wrap checks and unchecks (`95`, `96`).
- Pass: Alt+Space opens the window's system menu (Restore, Move, Size, Minimize, Maximize, Close; menu mode 0xC, `28`);
  Alt+F4 closes the app.
- Partly untested: AltGr typing, since the layout is en-US and changing it was off limits. What AltGr sends (left
  Ctrl+right Alt, with and without a letter) never opened or focused the bar (`31`, `32`); a right Alt alone acts as Alt
  on this layout (`30`), as Windows defines it.
- Pass: right-click on a row opens the in-page menu at the pointer (`40`); Shift+F10 and the Menu key open it under the
  cursor row with its first row highlighted and underlines on (`41`, `42`); it flips up at the bottom edge (`44`) and
  left at the right edge (`46`). No second menu (WebView2's own) appeared after the Menu key.
- Pass: the rename editor's right-click opens the in-page edit menu (Undo and Redo greyed on a fresh field, `51`). Copy
  put "3D Objects" on the clipboard; Paste replaced the name with the clipboard's text (`53`); Undo turned live after
  the paste and restored the name (`54`, `55`); Select all selected it (`56`); Cut emptied the field and the clipboard
  held the name (`57`). The rename stayed open throughout. Shift+F10 and the Menu key inside the field open the same
  menu under the field, once (`213`, `214`); the Settings search field gets it too, Undo live after typing (`211`).
  Shift+F10 and the Menu key inside the field open the same menu under the field, once (`213`, `214`); the Settings
  search field gets it too, Undo live after typing (`211`).
- Pass: Edit > Paste from the bar (Alt+E, P) while renaming pasted into the field and kept the rename open (`58`, `59`).
- Pass: Ctrl+W closes Settings, the queue, Keyboard shortcuts, and the viewer. Tab walks Settings (sidebar, then the
  Language select, `79`, `80`) and Keyboard shortcuts (the "Hide features" checkbox, `85`); the empty queue has no live
  control to Tab to, so untested there.
- Pass: the `SC_KEYMENU` guard on windows opened later: Alt and F10 in Settings, Alt+X in Settings, and Alt in the
  queue, Keyboard shortcuts, and the viewer leave menu-mode flags at 0.

Task 67 (browser keys):

- Pass: in Settings with "xyz" typed into its search, F5, Ctrl+R, Alt+←, Ctrl+plus, F7, F12, Ctrl+Shift+P, Ctrl+P, and
  Ctrl+J changed nothing: the text stayed, no zoom, no caret-browsing prompt, no DevTools, print, or downloads window
  (`78`, window list before and after). The same in the viewer (`94`). In the main window F5 opens Cmdr's Copy dialog,
  its own binding, and the rest did nothing of WebView2's (no new window).

Tasks 60 and 62 (look):

- Fail, fixed: Settings and the operation queue were see-through: the windows behind showed through the page (`70`,
  `81`). Their openers opened `transparent` with a clear fill whenever macOS's Reduce transparency was off, and the
  Sidebar material they asked for exists only on macOS. Keyboard shortcuts was opaque (`84`). Re-run on `968733ae7f`:
  both open opaque (`210`, `212`).
- Pass: the Fluent radio, switch, and select read as WinUI's (`70`). On `main` before task 40 the accent was Cmdr's gold
  under "System theme color"; on `a394bbdcfc` (task 40 merged) the radio, the swatch, and the selected segment draw
  exactly the system accent, `#0078D7` by pixel sample (`220`).
- Pass: text renders in Segoe UI. Segoe UI Variable isn't on this Windows 10, so the stack's `'Segoe UI Variable Text'`
  falls through to `'Segoe UI'`, as designed; the Variable cut needs a Windows 11 check. Not confirmed by a computed
  style read (the bridge failed), only by the glyphs.
- Untested: dark mode (the system is light, and switching it was off limits).

Task 40 (system look, merged mid-run as PR #1286, re-run on `a394bbdcfc`):

- Pass: `cargo nextest run -p cmdr --no-fail-fast system_look intl::` on the builder, 81 of 81 (13 `system_look`, 68
  `intl`), the first Windows run of those tests.
- Pass: the first system-look read at startup took 12.6 ms (`Windows look, read in 12.622ms` in the file log, accent
  `#0078d7`), with another agent's build running; recorded in `apps/desktop/src-tauri/DETAILS.md`.
- Pass: the controls' accent matches the system accent (above).

Task 34 leftovers:

- Pass: the volume switcher lists `C:` (no label) and `build (D:)` with NTFS and free space, and Servers (`100`).
- Pass: the path bar reads `C:\Program Files` (`102`).
- Open: at the drive root the path bar shows nothing after the volume chip (`101`), and the home and Downloads tabs read
  `~` and `~/Downloads`, with a `/` (`03`).

## Open problems, by severity

1. **High: the Copy dialog refuses every local copy** with "Filename can't be empty" under a destination of
   `~/Downloads/3D Objects`, Copy greyed (`77`). Likely `resolveTransferFilename`
   (`apps/desktop/src/lib/file-operations/transfer/transfer-dialog-utils.ts`) answering `null` on Windows, which the
   dialog words as an empty name: `~/` expands through `joinPath` onto the home path, and `splitNormalizedPath`'s
   `parsePath` may not read the result (a native home, or a mixed `C:/…` spelling). Not chased; local copy doesn't land
   on Windows yet anyway (#1225), but the message is also wrong for a `null` answer. Fixed (2026-11-21, #1292, PR
   #1304): the native home was the cause. The dialog read Tauri's `homeDir()` (`C:\Users\Admin`), so `~/Downloads/…`
   expanded to a path holding a `\`, which the app-path grammar refuses; it now reads the backend's `getHomeFolder`
   (`C:/Users/Admin`), as do the pane's breadcrumb `~` and the drop rules, which had the same bug.
2. **Low: Settings is slow to appear under load**: on `a394bbdcfc`, with another agent's build on both cores, Ctrl+,
   took more than 3.5 s to show Settings, and the main window went "Not responding" (a ghost window) for a few seconds
   the first time; Alt+F4 then took over 5 s to exit. Not seen on `4314b3cf1f` under a similar load; not attributed. Now
   #1307.
3. **Low: path bar spelling**: the drive root shows no segment, and `~`-paths keep `/` (task 34 leftovers).
4. **Low: the Copy dialog offers "Skip Mac-only hidden files (.DS_Store, .\_\*)" on a Windows-to-Windows copy** (`77`).
   Now #1305: a Mac-made stick or share carries those files on Windows too, so it needs a decision.
5. **Low: the Tauri MCP bridge connects but runs nothing on Windows** ("WebView execution failed" for `execute_js` and
   `manage_window list`), so agents can't read DOM state there. Now #1306.
6. **Untested**: AltGr typing (needs a layout with AltGr), dark mode, Segoe UI Variable (Windows 11), an audible beep,
   Tab in a non-empty queue.
