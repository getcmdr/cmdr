# The menu model on Windows and Linux

Evidence and decision for task 63 of `docs/specs/windows-port-plan.md`: where Cmdr's menus live on Windows and on each
Linux desktop, carried as values of the `menuModel` convention (`apps/desktop/src-tauri/src/platform/conventions.rs`).
Researched 2026-11-18. "Verified" means read from the page that day (Firecrawl scrape or search excerpt, quoted text);
"unverified" means from memory, a secondary source, or not checked on our own stack.

Recommended by the lead, 2026-11-18, and **decided by David the same day** (D5) after running the menu spike on Windows
11: the native modes worked but felt Win32-like, so Windows gets in-page menus matched precisely to Windows 11
(`docs/notes/windows-port/menu-spike.md` § Decision, which also rechecks this note's Windows facts against upstream;
read its "Upstream facts" before relying on the "Technical constraints" section below). The decision added three things
this note's research didn't have: the right-click menus go in-page on Windows too, access keys are underlines (no
keytips), and Windows' "Underline access keys" setting is honored. GNOME and KDE stand as researched.

## The decision in one screen

- **macOS**: `systemMenuBar`, unchanged (AppKit's bar through muda).
- **Windows**: `windowMenuBar`, a menu bar Cmdr draws in the page, under the native title bar, visible by default, in
  Windows 11's Fluent look on Windows 10 and 11 alike (tokens read from WinUI's theme resources). A tap of Alt or F10
  focuses it, Alt plus a title's letter opens that menu, and the same access-key engine as the dialogs underlines its
  letters, after Alt or other keyboard use as Win32 does, or always with Windows' "Underline access keys" on. The muda
  Win32 bar goes away, and the right-click menus move in-page onto the same menu component.
- **GNOME**: a new value, `primaryMenu`: no bar; a "Main menu" button (the `open-menu` icon) at the end of Cmdr's top
  row holds the bar's menus as submenus plus GNOME's standard group, and F10 opens it.
- **KDE**: `windowMenuBar`, the same in-page bar, visible by default, with a "Show menu bar" toggle; hidden, a hamburger
  button takes its place (KDE's `KHamburgerMenu` pattern).
- **Other Linux** (Xfce, Cinnamon, MATE, and the rest): `windowMenuBar`, as KDE.
- **Everywhere off macOS**: the command palette (`Ctrl+Shift+P`) stays the search-everything layer for all ~230
  commands, and the function key bar stays as TC users know it. One component draws the bar, one draws the menu button,
  both from `cmdr-menu`'s data, so every menu row stays one row in Rust.

## The research

### Windows: Microsoft's guidance

- **WinUI, Menu flyout and menu bar** (`learn.microsoft.com/en-us/windows/apps/develop/ui/controls/menus`, last updated
  2026-10-14, verified): "To show a set of multiple top-level menus in a horizontal row, use menu bar (which you
  typically position at the top of the app window)." It offers the control; it doesn't rank it against a command bar.
- **WinUI, Menus and context menus** (`…/controls/menus-and-context-menus`, last updated 2026-10-13, verified): "For
  apps that might need more organization or grouping, use a menu bar as a quick and simple way to show a set of multiple
  top-level menus in a horizontal row."
- **WinUI, Command bar** (`…/controls/command-bar`, last updated 2026-10-13, verified): command bars give "easy access
  to your app's most common tasks", primary commands moving to an overflow menu as the window narrows. It's framed for
  the most common few, not for a whole command set.
- **Win32 UX guide, Menus** (`learn.microsoft.com/en-us/windows/win32/uxguide/cmd-menus`, `ms.date` 2022-01-25,
  verified): for a primary window with "many menu items", "many menu categories", items that "apply to the entire
  program", and a menu that must "work for all users": "consider using a menu bar." Also: "Don't have more than 10 menu
  categories"; "Hide the menu bar instead of removing it completely, because menu bars are more accessible for keyboard
  users"; restore it from a "Menu bar" check item in View; "Assign access keys to all menu items. No exceptions."
- **Access keys** (`…/develop/input/access-keys`, `ms.date` 2024-06-11, verified in `platform-conventions.md`): WinUI
  shows keytips on Alt; "We recommend that you specify access keys wherever appropriate in your UI." Cmdr draws Win32's
  underlines instead (D5): they read the same in a menu bar and a dialog, and Windows' own "Underline access keys"
  setting governs them.
- Reading: Cmdr fits the Win32 guide's menu-bar case on every count (a primary window, seven categories, ~77 bar rows,
  app-wide commands, keyboard users). Nothing current tells a large keyboard-first app to drop its bar.

### Windows: what shipping apps do

- **File Explorer (Windows 11)**: a WinUI command bar (New, Cut, Copy, Paste, Rename, Share, Delete, Sort, View, and
  "See more" overflow), no menu bar; Alt shows keytips on the command bar and F10 reaches it (unverified; the "See more"
  overflow is verified by press coverage of its 24H2 bug, techradar.com and askvg.com). Commands stay discoverable
  through the right-click menu and the command bar's ~10 buttons. It has far fewer commands than Cmdr.
- **Total Commander**: classic Win32 menu bar (Files, Mark, Commands, Net, Show, Configuration, Start), a button bar,
  and the F-key bar at the bottom. F9 activates "menu above source window", F10 "Activate left menu or deactivate menu"
  (verified, ghisler.ch forum thread t=52262 and several shortcut lists quoting TC's help). Alt+letter opens a menu.
- **Files** (files.community): no menu bar; a customizable toolbar, an Omnibar with a command palette mode
  (`Ctrl+Shift+P`, or `>` in the address bar), and rebindable shortcuts (verified, files.community docs and the v3 and
  v4.0 release posts).
- **Double Commander**: a classic menu bar plus button bar and F-key bar, TC's layout (unverified, from memory).
- **Directory Opus**: menus are a "menu toolbar" in the lister, customizable like any toolbar; the viewer has a "Menu
  Bar" toggle (verified excerpt, docs.dopus.com viewer keys page); default lister keeps File/Edit/… menus at the top
  (unverified).
- **Visual Studio Code**: draws its own menu bar in the title bar on Windows and Linux. `window.menuBarVisibility`:
  `classic`, `visible`, `toggle` ("the menu bar is hidden and a single press of the Alt key makes it visible"),
  `compact` (moved into the Side Bar), `hidden` (verified, code.visualstudio.com Custom Layout page). Command palette
  for everything.
- **Windows Terminal**: no menu bar; tabs, a dropdown, and a command palette on `Ctrl+Shift+P` (verified,
  learn.microsoft.com/windows/terminal/command-palette).
- **Notepad (Windows 11)**: a WinUI menu bar (File, Edit, View) under tabs in the title bar, Fluent-styled, Alt shows
  keytips (unverified, from memory of the 2023+ redesign).
- **Office**: the ribbon, a command-bar family member, with keytips on Alt (verified in passing: Excel docs quoted in a
  search excerpt, "If you tap the ALT key on your keyboard you will see letters appear on the Ribbon").
- Pattern: apps with a small command set (Explorer, Terminal, Files) drop the bar for a command bar or palette; apps
  with a large one keep a bar (TC, Double Commander, Opus, VS Code, Notepad, Office's ribbon). Modern Microsoft apps
  draw theirs in their own UI framework (WinUI, Electron), not with Win32 `HMENU`.

### GNOME

- **HIG, Menus** (`developer.gnome.org/hig/patterns/controls/menus.html`, verified): "Most apps have primary menus"; the
  button uses `open-menu-symbolic`, "typically placed at the end … of the header bar"; tooltip and accessible label
  "Main Menu"; the standard group at its end: Preferences, Keyboard Shortcuts, Help, About App; no Close or Quit.
  Secondary menus hold a view's or item's actions. The HIG describes no menu bar.
- **HIG, Keyboard** (`…/guidelines/keyboard.html`, verified): "F10: Open primary or secondary menu"; "Menu / Shift+F10:
  Open context menu for focused location".
- **Nautilus (Files)**: header bar, a main menu and view menus, no menu bar (unverified on a GNOME page this time; the
  search hit a connection reset).
- **Cross-desktop keyboard-heavy apps under GNOME**: VS Code keeps its own bar (verified above); LibreOffice, GIMP, and
  Inkscape keep menu bars on GNOME (unverified, from memory). GNOME's own large apps (Builder) moved to header-bar menus
  plus a search popover (unverified).

### KDE

- **HIG, Layout and navigation** (`develop.kde.org/hig/layout_and_nav/`, modified 2026-11-12, verified): a hamburger
  menu only "if its contents can be kept to about 15 items or fewer. If you are tempted to add more than this, a
  hamburger menu is the wrong UI element for your app; instead place a real menubar between the titlebar and toolbar."
  "Use KHamburgerMenu to give a medium-to-large app both a menu bar and a hamburger menu", the hamburger showing "when
  the menubar is hidden". Don't put Quit or Minimize in a hamburger menu.
- **Dolphin**: hamburger by default with the menu bar hidden, "Show Menubar" in its menu and on `Ctrl+M` (the menu entry
  is verified, discuss.kde.org thread 4396; `Ctrl+M` is KDE's standard toggle, unverified on a KDE page).
- **Krusader**: a classic menu bar, TC-style layout and F-key bar (unverified, from memory).

### Technical constraints in Tauri 2

- **A Win32 bar doesn't hear Alt from WebView2.** With focus in the web content (always, in Cmdr), Alt and Alt+letter
  don't reach the host window's menu bar: `MicrosoftEdge/WebView2Feedback#336` ("Alt menus (mnemonics) do not work
  properly", open for years, verified) and `#468` (key forwarding to the host, verified). WebView2 added
  `CoreWebView2ControllerOptions.AllowHostInputProcessing` (SDK 1.0.3351, runtime 1.0.1901.177; Rick Strahl's post of
  2025-08-20, verified), but it routes those keys to the host BEFORE the page, so the page loses keys it wants; whether
  wry exposes it is unverified. A native bar would need that plus hand-routing to keep Cmdr's own Alt keys.
- **A lone Alt or F10 still starts Windows' menu loop with no bar at all.** `DefWindowProc` answers them with
  `WM_SYSCOMMAND`/`SC_KEYMENU` (`lParam` 0) and enters a modal menu loop; in a Tauri app that stalls IPC both ways until
  a click or Esc. Another Tauri app measured it and fixes it by answering a zero-`lParam` `SC_KEYMENU` itself
  (`copilotkit/openbot` `desktop/src-tauri/src/key_menu.rs`, verified source; unverified on Cmdr). Alt+Space (system
  menu) and Alt+letter still go to `DefWindowProc`.
- **Accelerators**: an in-page bar registers none; the frontend registry's keydown dispatch already runs every key
  (`bar_parity_test.rs` holds the bar's SHOWN keys to the registry). That removes the open question "does a Win32
  accelerator fire before WebView2" and the page-owned-keys hazard on Windows.
- **Browser keys** (#1115): WebView2 takes `F5`, `Ctrl+R`, `F3`, `F7`, `Alt+←/→`, and so on before the page until #1115
  turns them off. F10 and a lone Alt aren't in WebView2's browser-key list (unverified on a Microsoft page).
- **Dark mode and contrast themes**: a Win32 menu bar has no documented dark mode (apps hook undocumented `uxtheme`
  messages; whether muda's Windows bar follows dark mode is unverified). An in-page bar takes the theme tokens and
  `forced-colors` like every other surface (task 64).
- **Screen readers**: Chromium maps ARIA `menubar` / `menu` / `menuitem` / `menuitemcheckbox` to UI Automation's
  MenuBar, Menu, and MenuItem (unverified on a Microsoft page; standard Chromium behavior). `lib/ui/Menu.svelte` already
  carries these roles for VoiceOver. WebKitGTK maps them to AT-SPI for Orca (unverified). A native bar is exposed for
  free, which is the in-page bar's main cost.
- **Linux today**: muda builds a GTK menu bar per window. GTK takes a bar title's `Alt+<letter>` before WebKit, so
  `menu/mnemonic_mask/` switches the window's mnemonic modifier off while a modal surface is open, and Settings warns
  about recorded `Alt+<letter>` keys that collide with title mnemonics (`apps/desktop/src-tauri/src/menu/DETAILS.md` §
  "A modal surface takes the mnemonics away"). An in-page bar makes both unnecessary: the page sees every key, and a
  dialog owns Alt by construction.
- **Reuse already in the tree**: the browser demo draws the macOS bar in HTML from `cmdr-menu` through
  `cmdr-listing-wasm`'s `DemoMenuBar` (`apps/desktop/src/lib/demo/menu-bar.svelte.ts`, ~290 lines, plus its style), with
  keyboard walking and click lanes identical to the native events. `lib/ui/Menu.svelte` is the in-app menu primitive
  (submenus, check items, ARIA roles). `lib/ui/access-keys.ts` allocates letters from rendered labels by the Linux menu
  bar's rule (`menu/mnemonics.rs`) and draws underlines (on Windows by Win32's keyboard-cue rule since task 63).
  `cmdr-listing-wasm/src/context_menu.rs` already answers every right-click menu's rows for the demo, the shape the
  in-page context menus on Windows follow.

## Per desktop: what the user gets

### Windows: `windowMenuBar`, drawn in the page

- **What they see**: under the native title bar, a menu bar in Windows 11's look (File, Edit, Select, View, Go, Tab,
  Help: today's Windows rows of `cmdr-menu`), then the tabs and panes, then the function key bar. Menus open as Fluent
  flyouts (acrylic, rounded, each row's shortcut on the right in the secondary text color); near the window's edge they
  flip or scroll, since an in-page menu can't leave the window.
- **Where the ~230 commands live**: ~77 in the bar (the curated set, as on macOS); all ~230 in the command palette
  (`Ctrl+Shift+P`, also View > Command palette), each with its key; the F-keys on the function key bar; the Keyboard
  shortcuts window lists every binding. Right-click menus are in-page too, the same component in the same look.
- **Keys**: Alt tapped alone (press, release, no other key) or F10 focuses the bar's first title with its letters
  underlined; arrows walk it, Enter or Down opens, Esc closes a menu then leaves the bar and gives focus back. Alt held
  plus a title's letter opens that menu (`Alt+F` File). Letters come from `access-keys.ts`, the dialogs' engine, so the
  bar underlines them exactly as dialogs do: hidden until Alt or another key is used, as Win32 does, and always shown
  with Windows' "Underline access keys" on. Every row inside a menu has its own letter, per the Win32 guide. Alt+Space
  stays the system menu; Alt+F4 stays the quit gate; Shift+F10 stays the file context menu.
- **With a dialog open**: the bar is inert (the existing menu-dialog gate), so Alt belongs to the dialog: its underlines
  show, and the multi-rename sheet's `Alt+F` / `Alt+E` / `Alt+G` stay its options. No mask, no collision.
- **A Total Commander user**: a bar at the top and the F-key bar at the bottom, F10 and Alt+letter into the menus, F3
  through F8 doing what TC does. TC's F9 ("menu above the active pane") has no equivalent; F9 stays free.
- **A File Explorer user**: a menu bar as in Notepad and Paint rather than Explorer's icon command bar, the usual
  `Ctrl+C`/`Ctrl+V`/`F2`/`Delete`, right-click menus that look like Windows 11's, and the palette when they look for
  something.
- **Hidden bar** (View > Show menu bar, a setting): the bar collapses; Alt or F10 shows it until focus leaves, as the
  Win32 guide and VS Code's `toggle` do.

### GNOME: `primaryMenu`

- **What they see**: no bar. A "Main menu" button (`open-menu` icon, tooltip and accessible name "Main menu") at the
  trailing end of Cmdr's top row. It opens a menu with the seven bar menus as submenus, then the HIG's standard group at
  the end: Settings, Keyboard shortcuts, Help, About Cmdr. Its File submenu drops Quit (the HIG: no Close or Quit
  there).
- **Keys**: F10 opens the main menu, the GNOME HIG key; arrows and Enter walk it; Alt+letter access keys work inside it
  (the HIG asks for them on labelled components). The command palette is the fast path, as GNOME Builder's search is.
- **Show menu bar**: the same setting, off by default here; on, GNOME gets the KDE layout. A TC refugee on GNOME gets
  their bar back in one click.

### KDE: `windowMenuBar`, with the hamburger fallback

- **What they see**: the same in-page bar as Windows, in the Linux neutral theme, visible by default (the KDE HIG: over
  ~15 items, a real menubar). Hidden through View > Show menu bar, a hamburger button (`application-menu` icon) takes
  its place with the full tree, KHamburgerMenu style.
- **Keys**: Alt+letter opens a menu with underlines as KDE draws them; F10 focuses the bar (unverified as a KDE
  standard; harmless, and it matches GTK and Windows). `Ctrl+M`, KDE's toggle, is Multi-rename in Cmdr's Linux keymap,
  so the toggle ships with no default key (bindable in Settings).

### Other Linux: `windowMenuBar`

The KDE layout. Xfce, Cinnamon, and MATE apps keep menu bars, and their users expect one (unverified per desktop HIG;
Xfce and MATE have no current HIG covering it).

## Alternatives rejected

- **Keep muda's native bar on Windows** (today's state). Free to keep and exposed to UI Automation without work, but it
  can't hear Alt or Alt+letter from WebView2 (#336) without `AllowHostInputProcessing`, which then steals keys from the
  page; dark mode needs undocumented hooks; it looks like Windows 7 next to Windows 11's own WinUI menu bars; and it
  would be a second access-key system beside the dialogs' own. Its one strong point, a11y for free, is matched by ARIA
  roles Chromium maps to UIA, checked in task 64. The spike (2026-11-18) confirmed it: with a ~40-line key bridge the
  keys work, and it still looked Win32 to David, as did the hybrid's native popups.
- **WinUI 3 menus through XAML Islands** (the real Windows 11 control). `windows-rs` has no WinUI 3 bindings, it needs
  the Windows App SDK runtime installed beside the app, and it adds a third input owner beside WebView2 and the host
  window. VS Code, Slack, and the new Teams draw their own menus on Windows too.
- **A command bar instead of a menu bar on Windows** (Explorer's model). Holds ~10 commands; Cmdr has ~230. The overflow
  would become a worse menu bar, and TC users lose F10 and Alt+letter. A command bar can come later as an optional
  toolbar beside the bar, as the Win32 guide says toolbars "work great together with menu bars".
- **Palette only, no menu** (Windows Terminal, Files). Fine for a few dozen commands and power users; hostile to
  discovery for everyone else, and the Win32 guide wants a menu that "work[s] for all users".
- **A bar on GNOME by default**. What LibreOffice and VS Code do, but it's exactly the "cross-platform app" look the
  design principles reject; GNOME's own guidance names the primary menu and F10. The setting keeps the bar one click
  away.
- **A hamburger by default on KDE** (Dolphin's default). The KDE HIG says over ~15 items it's the wrong element; Cmdr's
  bar has ~77.
- **Keep the GTK bar on Linux and draw only Windows'**. Two bar implementations, the mnemonic mask kept alive, GNOME
  still needing an in-page menu, and Linux's access keys working differently from its dialogs'.
- **Merge the bar into a custom title bar** (VS Code, Notepad's tabs). Contradicts the decided native decorations (Phase
  7 "Window chrome"); revisit only with that decision.

## Build cost and reuse

- **Kept as is**: `cmdr-menu`'s bar data, ids, labels, enabled fold, sort checks, and every context menu; the snapshot
  tests; `bar_parity_test.rs` (its "registered" checks narrow to macOS, its "shown" checks stay for all three); muda for
  macOS's bar and the right-click popups on macOS and Linux; the menu-dialog gate; `MenuItemInputs` as the one enabled
  rule.
- **Promoted**: the demo's HTML bar (`lib/demo/menu-bar.svelte.ts`, keyboard walking, click lanes) becomes a `lib/ui`
  bar the desktop and the demo share, built on `Menu.svelte` and `access-keys.ts`; the demo then draws the visitor's own
  model instead of always macOS's.
- **New**: a desktop command answering the bar the way `DemoMenuBar` does (rows, resolved labels, shown keys, enabled
  and checked state) plus a change event; an in-page answer for each right-click menu on Windows; the Windows 11 menu
  tokens; the main-menu button; the `primaryMenu` and `contextMenus` convention values; a show-menu-bar setting; the
  "Underline access keys" setting read; a Windows `SC_KEYMENU` guard.
- **Removed**: muda's bar on Windows and Linux (`menu_bar_builder.rs` builds it on macOS only), `menu/mnemonic_mask/`
  and its IPC command, `menu-mnemonics-capture.svelte.ts`, `get_menu_bar_title_mnemonics` and Settings' title-mnemonic
  warning (replaced by the bar's own letters reserving around a scope's Alt keys, as `keyScope` does for dialogs), and
  `menu-mnemonic-mask.spec.ts` (step 6 renamed it `menu-bar.spec.ts`, which now drives the in-page bar).
- Steps and sizes: the plan's task 63 entry.

## Left open (QA, not decisions)

- Whether a lone Alt and F10 reach the page's keydown in WebView2 before `SC_KEYMENU` (the openbot source says they do);
  task 55.
- Whether Narrator, NVDA, and JAWS announce the in-page bar and menus as menus, and Orca on WebKitGTK; task 64 and David
  (high contrast and Narrator).
- Whether Plasma's global menu applet matters to Cmdr's KDE users: an in-page bar can't export to it (whether muda's GTK
  bar did is unverified).
