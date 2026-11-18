# The Windows menu spike

What decides D5 (`docs/specs/windows-port-plan.md`): David runs `tools/menu-spike/` on a Windows 10 and a Windows 11
laptop and fills the results table below. The research recommendation it tests is in
`docs/notes/windows-port/menu-model.md`; this note rechecks that note's Windows facts against upstream first.

## The three modes

- **A. Native bar**: muda's Win32 menu bar, attached by Tauri as Cmdr's bar is today on Windows, with mnemonics
  (`&File`) and the registered accelerators. The **Key bridge** switch adds the fix a native bar would ship with:
  WebView2's `AcceleratorKeyPressed` event forwards a lone Alt, F10, and Alt+(title letter) to the bar as `SC_KEYMENU`.
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

### Unknown until the spike runs

- Whether a lone Alt, F10, and Alt+letter reach the native bar with focus in the page, without the bridge (they'd come
  through WebView2's own `DefWindowProc` call, if it makes one; the openbot `key_menu.rs` finding in the research note
  says a lone Alt and F10 do produce `SC_KEYMENU`).
- Whether the bridge's `SC_KEYMENU` actually moves keyboard focus into the bar, and back to the page on Escape.
- Whether a native popup opened from the page (mode B) and the right-click menu follow dark mode and high contrast on
  each Windows version, and how the Win32 bar looks next to Windows 11's own apps.
- What AltGr combos do on a layout that has AltGr (Hungarian on both laptops): they arrive as Ctrl+Alt+key, so a bar
  that reads them as Alt+letter would steal characters.
- Whether Narrator reads mode C's ARIA `menubar` as a menu bar.

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

1. **Native bar (A)** if Alt, F10, Alt+letter, and Escape work (with the bridge if needed: it's about 60 lines), no key
   fires twice once the bar's accelerators are display-only, and the dark and contrast looks are acceptable. D5 then
   flips to muda's bar on Windows; Linux and GNOME are decided separately.
2. **Hybrid (B)** if the native bar strip is the only problem (the keys or its look) but native popups are fine.
3. **In-page (C)** only if both fail: D5 stands as researched.

## What the agent observed without a screen

Pending: the `--selftest` run on mami (Windows 10 22H2, Hungarian locale).
