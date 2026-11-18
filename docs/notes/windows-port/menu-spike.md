# The Windows menu spike

What decides D5 (`docs/specs/windows-port-plan.md`): David runs `tools/menu-spike/` on a Windows 10 and a Windows 11
laptop and fills the results table below. The research recommendation it tests is in
`docs/notes/windows-port/menu-model.md`; this note rechecks that note's Windows facts against upstream first.

## The three modes

- **A. Native bar**: muda's Win32 menu bar, attached by Tauri as Cmdr's bar is today on Windows, with mnemonics
  (`&File`) and the registered accelerators. The **Key bridge** switch adds the fix a native bar would ship with:
  WebView2's `AcceleratorKeyPressed` event forwards F10 and Alt+(title letter) to the bar as `SC_KEYMENU`.
- **B. Hybrid**: a thin in-page strip of menu titles; Alt or F10 focuses it, Alt+letter opens a menu, Left and Right
  walk it. Each title opens a native popup menu under it, the same kind as the right-click menu.
- **C. In-page bar**: a page-drawn bar and dropdowns, deliberately unpolished. It shows the key routing, not the final
  look.

In every mode a right-click (or Shift+F10 on a list) opens the app's real file context menu, natively.

The switches: **SC_KEYMENU guard** (on in B and C) swallows the `SC_KEYMENU` Windows sends for a lone Alt, F10, or
Alt+letter, which would otherwise start its menu loop with no bar to open; **WebView2 browser keys** turns WebView2's
own keys (F5 and Ctrl+R reload, Alt+Left goes back) on and off; **Page owns F-keys, Ctrl+R, Alt+arrows** makes the page
call `preventDefault` on them, as Cmdr's shortcuts do.

The log's layers, newest first: `page keydown` (the page saw the key), `webview2 accel` (WebView2 told the host about
it), `host` (Windows sent `SC_KEYMENU`, a menu loop started or ended, a menu opened), `menu event` (a native menu item
or accelerator fired), `bridge`, `native popup`, and `in-page menu`. A "window blur" line means the page lost the
keyboard, usually to a native menu.

## Upstream facts (checked 2026-11-18)

Versions in our `Cargo.lock`: tauri 2.12.5, tauri-runtime-wry 2.12.3, wry 0.57.0, muda 0.20.0, tao 0.37.1, webview2-com
0.39.1. Latest upstream that day: wry 0.57.2, muda 0.21.2 (changelogs read through `gh api`). Sources read through `gh`
(issue and PR state, comments) and in the crates' source in `~/.cargo/registry`.

### Confirmed

- **Native accelerators don't fire while the page has focus.** Tauri translates muda's accelerators in the host's
  message loop (`tauri-2.12.5/src/app.rs`, `msg_hook` calling `TranslateAcceleratorW`), but with focus in WebView2 the
  keys go to WebView2's browser process, never through that loop. Open upstream: `tauri-apps/wry#451` ("CustomMenuItem
  accelerators don't trigger on_menu_event on Windows", open since 2021, last comment 2025-12-17: "Still an issue") and
  `tauri-apps/muda#107` (Alt+C doesn't fire in the wry example, open, marked a duplicate of #451). For Cmdr this matters
  less than it sounds: the page's own keydown dispatch already runs every shortcut, so a native bar's keys can be
  display-only, as macOS's popups' are.
- **The host can't opt into seeing WebView2's input yet.** WebView2's `AllowHostInputProcessing` (stable since SDK
  1.0.3351.48) routes input through the host's message loop, which would make accelerators and mnemonics work natively.
  wry 0.57 builds the controller options without it (`wry-0.57.0/src/webview2/mod.rs`, `create_controller`), and it can
  only be set at creation. Upstream: `tauri-apps/wry#1836` (feature request, opened 2026-09-09) and PR
  `tauri-apps/wry#1850` ("allow host input processing", opened 2026-09-16, open, no maintainer review as of 2026-11-18).
  Its own warnings: Tab leaves the webview before the page sees it (Rick Strahl's report in
  `MicrosoftEdge/WebView2Feedback#468`, 2025-07-10), and two open WebView2 bugs with it on, `#5375` (selection
  scrolling) and `#5613` (drag and drop blocks clicks).
- **The native bar and popups do have a dark mode.** muda 0.20 draws a dark Win32 menu bar through the undocumented
  `uxtheme` UAH hooks (`muda-0.20.0/src/platform_impl/windows/dark_menu_bar.rs`, a port of ppsspp's `UAHMenuBar` and
  `win32-darkmode`), with `MenuTheme::Auto` following Windows, and Tauri passes the window's theme to it (`set_theme`
  updates it). Popup menus (right-click, and mode B) go dark through tao's `SetPreferredAppMode(AllowDark)` on Windows
  10 1903 and later (`tao-0.37.1/src/platform_impl/windows/dark_mode.rs`). Recent fixes: dark bar text truncation (muda
  #357, 2026-05-20), icon items in dark mode (muda #434, 2026-11-03, after our 0.20.0). The research note's "no
  documented dark mode" is true of Microsoft's docs, not of muda.
- **What the research cited for Alt is about a different menu.** `MicrosoftEdge/WebView2Feedback#336` ("Alt menus
  (mnemonics) do not work properly", open since 2020-07-14, labels "feature request" and "tracked", last activity
  2022-05-10) reproduces with a WinForms `MenuStrip`, a managed control that handles mnemonics in the .NET message loop.
  muda's bar is a real Win32 `HMENU`, opened by `DefWindowProc` answering `WM_SYSCOMMAND`/`SC_KEYMENU`, which tao passes
  through (`tao-0.37.1/src/platform_impl/windows/event_loop.rs`, `WM_SYSCHAR` and `WM_SYSCOMMAND` go to
  `DefWindowProc`). So #336 neither proves nor disproves Alt on our bar; the spike measures it.
- **A WinUI or Fluent menu isn't coming from upstream soon.** The one request, `tauri-apps/muda#167` ("Support WinUI
  Styles Menu in windows 11", 2024-03-17), has no replies and no plan; it notes `windows-rs` dropped WinUI 3 XAML
  support. Nothing on Tauri's side mentions one.

### Still unknown after the agent's run (David's laptops answer these)

- Whether native popups (the bar's menus, mode B, the right-click menu) go dark when WINDOWS is in dark mode. With only
  the window forced dark, they stay light (below).
- High contrast, and how the Win32 bar looks next to Windows 11's own apps.
- AltGr on a layout that has it (mami's console runs an English layout, so the selftest's right Alt was a plain Alt).
  AltGr arrives as Ctrl+Alt+key; the bridge skips a key while Ctrl or right Alt is down, but that's untested.
- Narrator, in every mode.
- Whether the bridge's Shift check holds with real typing: under `SendInput`, Shift was already up by the time
  WebView2's event arrived, so the bridge read Shift+F10 as F10 and opened the bar as well as the context menu.

## What the agent observed on mami (Windows 10 22H2, no screen)

The `--selftest` ran three times in mami's console session (a one-off scheduled task as the signed-in `admin`, removed
afterwards): light, light after the fixes below, and with the window forced dark. Screenshots work with the lid closed
(1600 × 900). Logs and shots aren't kept in the repo.

- **Mode A, stock (what Cmdr ships today)**:
  - A lone Alt opens muda's bar: WebView2 passes the unhandled Alt release to `DefWindowProc`, which sends `SC_KEYMENU`,
    and "File" highlights. Escape ends the menu loop and the page has the keyboard again.
  - F10 does the same, but only when the page doesn't `preventDefault` it. The first run's page did (as a Cmdr F-key
    would), and F10 then did nothing.
  - **Alt+letter doesn't work**: Alt+F and Alt+E never reach Windows (no `SC_KEYMENU`), so no menu opens.
  - **Accelerators don't fire**: Ctrl+Shift+C (Copy path, registered on the bar) logged only the page's keydown, no menu
    event. So the bar's keys never double-fire, and never work on their own either: the page has to own them.
- **Mode A with the key bridge**: Alt+F and Alt+E open the right menu, Right walks to the next menu, Escape returns to
  the page, and the page no longer sees the letter. F10 through the bridge works too. Forwarding a lone Alt as well
  opened the menu loop twice, so the bridge leaves it to Windows now. The bridge is about 40 lines on WebView2's
  `AcceleratorKeyPressed`.
- **Mode B**: Alt and F10 focus the strip (the page's `preventDefault` on the Alt release means Windows sends no
  `SC_KEYMENU` at all), Alt+F opens File's native popup under its title, and Right closes it and opens Edit's, via a
  `WH_MSGFILTER` hook. It looks like a real bar's menu.
- **Mode C**: Alt+F, F10, and the arrows work as written; it looks like a web page, as expected unpolished.
- **Browser keys**: with WebView2's browser keys on, the page's `preventDefault` still wins: F5, Ctrl+R, and Alt+Left
  didn't reload or go back. Ctrl+Shift+P (not prevented by the page) briefly took focus from the window twice, likely
  WebView2's own print or similar UI; #1115's browser-keys-off covers it.
- **Shift+F10** opens the native file context menu in every mode (the page's own handler; WebView2 opened a second one
  until the page called `preventDefault`, fixed).
- **Dark, window forced dark, Windows light**: the title bar and muda's bar strip go dark, but every native popup (the
  bar's menus, the right-click menu) stays light. tao asks Windows for `AllowDark`, which follows the system's app mode,
  so with Windows itself dark they should go dark too: step 9 checks it.

## Get it

- **The build**: `~/Downloads/cmdr-menu-spike/menu-spike.exe` on David's Mac, one 9.7 MB file built on mami (release,
  `opt-level` 1). Copy it to each laptop (USB stick, a share, or a cloud folder) and double-click it. SmartScreen warns
  that it's unsigned: "More info", then "Run anyway".
- **WebView2**: it needs the Evergreen WebView2 runtime, which Windows 11 ships and Windows 10 gets through Windows
  Update and with Edge (mami has 157.0.4341.60). Nothing else: the WebView2 loader is linked in, and it installs
  nothing. WebView2 keeps its cache in `%LOCALAPPDATA%\com.getcmdr.menuspike`; delete that folder afterwards.
- **Rebuild**: `tools/menu-spike/README.md`.

## Test script (about 15 minutes per laptop)

Copy `menu-spike.exe` to the laptop and double-click it. Clear the log before each row, and keep focus in the left list
unless a step says otherwise. For each mode, A (also with the key bridge on), B, and C:

1. **Alt tap**: press and release Alt. Look: does the bar's first title highlight, with letters underlined? Then Escape:
   does the list get focus back (its cursor row turns accent-colored)? Log: is there a `host` "SC_KEYMENU" line, a "menu
   loop started" line?
2. **F10**: same as 1.
3. **Alt+F, Alt+E, Alt+V**: does the right menu open? Arrow Right and Left: does it walk to the neighbor menus? Pick an
   item with Enter: does a `menu event` or `in-page menu` line appear?
4. **Escape twice** from an open menu: closed and back in the list?
5. **Accelerators** with focus in the list: F5, Ctrl+R, Alt+Left, Ctrl+Shift+C (Copy path), Ctrl+Shift+P (command
   palette). Log: `page keydown` only (good), plus a `menu event` (fires twice in the real app), or a page reload (the
   log empties and says "loaded"; bad)? Repeat F5 and Alt+Left with "WebView2 browser keys" off.
6. **Text field**: click it, type, then Alt+F. Does the menu open and does the field keep its text?
7. **AltGr**: switch to a layout with AltGr (Hungarian), click the text field, and type AltGr+Q (`\`) and AltGr+V (`@`).
   The character should appear and no menu should open.
8. **Right-click** a row, then Shift+F10 on a row: the native context menu at the right spot?
9. **Theme**: set Windows to dark (Settings > Personalization > Colors), and look at the bar, an open menu, and the
   context menu in each mode; then the in-app Theme picker set to Light and Dark. Then a contrast theme (Left Alt+Left
   Shift+Print Screen) and the same look.
10. **Narrator** (Ctrl+Win+Enter): in each mode, Alt or F10 then arrows. Does it say "menu bar", the title, "menu item"?

## Results

One row per mode and laptop. Use ok, no, or a few words.

| Laptop | Mode     | Alt tap | F10 | Alt+letter | Arrows between menus | Esc back to list | F5 / Ctrl+R / Alt+Left | Ctrl+Shift+C fires twice? | AltGr types | Dark | High contrast | Narrator | Looks native enough? |
| ------ | -------- | ------- | --- | ---------- | -------------------- | ---------------- | ---------------------- | ------------------------- | ----------- | ---- | ------------- | -------- | -------------------- |
| Win 10 | A        |         |     |            |                      |                  |                        |                           |             |      |               |          |                      |
| Win 10 | A+bridge |         |     |            |                      |                  |                        |                           |             |      |               |          |                      |
| Win 10 | B        |         |     |            |                      |                  |                        |                           |             |      |               |          |                      |
| Win 10 | C        |         |     |            |                      |                  |                        |                           |             |      |               |          |                      |
| Win 11 | A        |         |     |            |                      |                  |                        |                           |             |      |               |          |                      |
| Win 11 | A+bridge |         |     |            |                      |                  |                        |                           |             |      |               |          |                      |
| Win 11 | B        |         |     |            |                      |                  |                        |                           |             |      |               |          |                      |
| Win 11 | C        |         |     |            |                      |                  |                        |                           |             |      |               |          |                      |

## Decision rule

1. **Native bar (A)** if Alt, F10, Alt+letter, and Escape work (with the bridge if needed: it.s about 40 lines), no key
   fires twice once the bar's accelerators are display-only, and the dark and contrast looks are acceptable. D5 then
   flips to muda's bar on Windows; Linux and GNOME are decided separately.
2. **Hybrid (B)** if the native bar strip is the only problem (the keys or its look) but native popups are fine.
3. **In-page (C)** only if both fail: D5 stands as researched.

## Seeing mami's screen

mami runs lid closed, but its console session is live (`admin` is signed in at the console, and screen captures from it
work), so a remote view shows the real desktop:

- **Remote Desktop**: enabled (`fDenyTSConnections` 0, `TermService` running, NLA on), listening on 3389, and the
  built-in "Remote Desktop - User Mode" firewall rules allow any address on every profile, so
  `mami-laptop.tail1b22.ts.net:3389` answers over the tailnet (checked 2026-11-18). Windows App on the Mac connects as
  `Admin`, but it needs that account's password, which isn't in the secrets store; and signing in over RDP takes the
  console session over (the lid-closed console locks), which is fine for this.
- **Chrome Remote Desktop**: its host service (`chromoting`) runs, but the infra repo's mami-laptop notes found its
  registration dead on 2026-11-08; re-registering through `remotedesktop.google.com/headless` brings it back.
- **No view at all**: `menu-spike.exe --selftest <dir> --log <dir>\log.txt`, started in the console session (the agent
  used a one-off scheduled task), saves a screenshot per step and the full log.
