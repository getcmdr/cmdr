# Platform details

## How the values arrive

- `src-tauri/src/tauri_builder.rs` registers a tiny plugin (`cmdr-platform`) whose only job is a `js_init_script`, built
  by `apps/desktop/src-tauri/src/platform/frontend.rs`:
  `Object.defineProperty(window, '__CMDR_PLATFORM__', { value: '<os>' })`, and the same with `Object.freeze({…})` for
  `__CMDR_CAPABILITIES__` and `__CMDR_CONVENTIONS__` (§ Conventions). Tauri runs a plugin's init script in every webview
  the app creates, before the page's own scripts: the config's main window and each window the frontend opens later
  (viewer, Settings, the queue, the help and debug windows). So no window can render, or evaluate a module, without
  them.
- Both properties are read-only (`defineProperty` without `writable`, the capabilities frozen too), so nothing in the
  page can change its mind later.
- `getPlatform()` reads its global on every call and checks it names one of the three platforms. A missing or unknown
  value answers `FALLBACK_PLATFORM` (`'linux'`).
- `platformCapabilities()` / `hasCapability()` read theirs on every call, field by field: a flag holds only where the
  injected value says exactly `true`. A missing global, a missing field (an older or newer backend), or junk reads as
  "not offered", so a surface hides rather than shows by accident. `NO_CAPABILITIES` is the all-`false` set.
- `hooks.client.ts` calls `applyPlatformAttributes()` at module evaluation, which every window runs before its first
  route renders, so `:root[data-platform=…]` and `:root[data-overlay-title-bar]` selectors hold from the first paint.

Decision: an init script, not an IPC command. Why: several modules read these AT MODULE LOAD (the settings registry, for
one), and an awaited `invoke` can't answer before they evaluate. The other option, a `tauri-plugin-os` call, is async
for the same reason, adds a dependency, and knows nothing about Cmdr's features.

Decision: one platform value per process, typed in TS, and Rust's own `std::env::consts::OS` on the backend. Why: Rust's
names for the three targets are exactly the `Platform` values, so nothing maps or mirrors them, and the crate root
refuses every other target (`docs/guides/platform-seams.md` § Unknown targets) instead of silently reporting "linux".

## Capabilities

The frontend used to ask "is this macOS?" wherever a feature exists only there (Quick Look, Full Disk Access, the Finder
extension, …), which says nothing about Windows and hides each feature by accident rather than on purpose. Now the
backend says what exists, and a surface asks for exactly the thing it needs.

- **Source of truth**: `PlatformCapabilities` in `apps/desktop/src-tauri/src/platform/capabilities.rs`, one documented
  `bool` per OS-provided surface, named after the UI that asks. Each OS arm of the platform seam (`platform/macos.rs`,
  `platform/linux/mod.rs`, `platform/windows.rs`) answers every field as a struct literal, so a new field fails to
  compile until each OS has decided. The Linux arm's `imageIndexing` follows the `playwright-e2e` feature: the E2E build
  runs a fake image engine there (`cmdr-index`'s `media_index/scheduler/lifecycle.rs::platform_backends`), and nothing
  else does.
- **Every arm answers on purpose, Windows included.** The macOS-only surfaces with no other OS behind them are
  `localNetworkPermission` (a TCC prompt; Windows' firewall prompt is its own thing), `localAi` (the bundled
  llama-server is the macOS ARM64 build; the backend's `is_local_ai_supported` narrows it on the machine, so an Intel
  Mac still sees the option greyed out), `safeSaveFiles` (only NSDocument writes `.sb-` files), `appBundles` (no
  LaunchServices elsewhere; `cmdr-fs`'s `folder_names_make_packages` is the backend's matching per-OS fact), `appMenu`
  (an OS app menu that owns Quit, Hide, Hide others, and Show all: off macOS the Hide trio doesn't exist and Quit is
  Cmdr's own command, `lib/commands/DETAILS.md`), and `homebrewCask` (the macOS distribution). `fullScreen` is `true` on
  all three (`view.toggleFullScreen`, F11 off macOS), which brings the Escape-exits-full-screen setting and its hint
  everywhere. `hideableMenuBar` is `true` off macOS, where the page draws the bar (D5): View > Show menu bar, the
  `appearance.menuBar` setting, and `view.toggleMenuBar` exist there
  (`apps/desktop/src/lib/ui/command-menu/menu-layout.ts`). `globalShortcuts` is `true` on macOS and Windows)
  (`RegisterHotKey`) and, on Linux, only on an X11 session, read once at startup (`XDG_SESSION_TYPE`, else
  `WAYLAND_DISPLAY` unset): `global-hotkey` grabs keys through X11 only, so on Wayland the grab fires only inside
  XWayland windows. The backend asks it too before registering the hotkey (`downloads/runtime.rs`).
- **Decision/Why: `homebrewCask` is a capability, though it's a distribution fact.** It fits "named after the UI that
  asks" poorly and will want siblings (winget, Flathub) once those channels exist; the alternative was one
  `distribution_note` line shown everywhere, with a branch per OS. Kept as a capability because only macOS has a channel
  to star today; revisit when a second channel lands.
- **`memoryDiagnostics` and `cpuDiagnostics` are the flags no UI asks.** Each decides whether its MCP tool
  (`memory_diagnostics`, `cpu_diagnostics`) exists on this OS (`apps/desktop/src-tauri/src/tools/DETAILS.md` § The
  table), and they're here so every arm answers them. Two flags because the readers differ per OS: Linux has the CPU
  census (`/proc/self/task`) but no memory-map reader yet. `colorTags` gates the `tag` tool the same way.
- **Typed in TS from Rust**: `ipc.rs` exports the struct with `.typ::<PlatformCapabilities>()`, so `bindings.ts` carries
  it like any command's type, and `Capability` is `keyof` it. No IPC command returns it; the init script is the only
  channel.
- **What asks**: the command registry (`needsCapability` on a command: every listing surface skips it and it gets no
  default key, `lib/commands/DETAILS.md` § "OS-feature commands"), settings (`needsCapability` on a definition or a
  searchable row drops it from the nav tree, search, Advanced, the MCP settings resource, and every section's
  `shouldShow`), the menu bar (`macos_only` rows, pinned to their capability per OS by
  `apps/desktop/src-tauri/src/menu/capability_rows_test.rs`), the nudges (`NUDGE_CAPABILITY` maps each offer to its
  own), Full Disk Access everywhere it shows, the F9 terminal slot, the Services selection push, and the overlay title
  bar.
- **What doesn't**: `isMacOS()` stays where the question really is the platform. Key glyphs and the combo grammar
  (`key-capture.ts`, the shortcut filter, the favorites tooltip), the updater's macOS flow, and the old-macOS notice. ❌
  Never copy: wording that differs per OS is one catalog value with a branch per OS (`../intl/DETAILS.md` § Platform
  variants), and `cmdr/no-platform-copy-branch` refuses an OS test that picks a string.

Decision: booleans, not a list of names or an OS-file-manager name. Why: a flag per surface is exhaustive in both
languages (a missing answer is a compile error in Rust and a type error in the demo's table), and the per-platform names
belong to copy, which an ICU `select` on the platform answers.

## Conventions

Capabilities say what exists; conventions say how the platform's own apps behave, so a `lib/ui` primitive can follow
them without asking which OS it's on. A "Windows" check would leave Linux on one guess, and Linux splits: GNOME orders
dialog buttons like macOS, KDE like Windows.

- **Source of truth**: `PlatformConventions` in `apps/desktop/src-tauri/src/platform/conventions.rs`, one typed enum per
  question: `dialogButtonOrder` (`cancelFirst` | `affirmativeFirst`), `enterActivates` (`defaultButton` |
  `focusedButton`), `cancelKeys` (`escape` | `escapeOrCommandPeriod`), `accessKeys` (`none` |
  `underlinesAfterKeyboardUse` | `underlinesWhileAltHeld`: when the underlines show, Win32's keyboard-cue rule or GTK's
  and Qt's held Alt, `lib/ui/keyboard-cues.ts`), `menuModel` (`systemMenuBar` | `windowMenuBar` | `primaryMenu`),
  `hiddenMenuBar` (`none` | `showsOnAltOrF10` | `f10OpensMainMenu`: what stands in for a bar the user hid, beside the
  main-menu button), `contextMenus` (`native` | `inPage`), `dropModifiers` (`optionCopiesCommandMoves` |
  `controlCopiesShiftMoves`: the keys that force a drop's copy or move, read by `file-explorer/drag/drop-operation.ts`),
  `standardFolderIcons` (`accentTinted` | `ownColors`: whether Cmdr gold may recolor Documents and friends, read by
  `file-explorer/selection/FileIcon.svelte`; Windows' are full-color artwork, #1479), `screenReaders` (most used first),
  `linuxDesktop` (`gnome` | `kde` | `other`, `null` off Linux), and `appFile` (`appBundle` | `exe` | `executable`: what
  a "Choose an app…" picker filters on, `settings/sections/app-choice-options.ts`). Each value and its guideline, with
  dates and what stayed unverified: `docs/notes/windows-port/platform-conventions.md`.
- **`pageDrawsMenuBar()`** is the one reading of `menuModel` the windows mount their bar on: true for every model but
  `systemMenuBar`. Whether that's a bar or a main-menu button is `#lib/ui/command-menu/menu-layout.ts`'s `menuLayout`
  (the menu model, `hiddenMenuBar`, and the "Show menu bar" setting). The backend asks the same question of its own row
  (`menu::builds_native_bar`), so the page and the native bar never both draw, past a `CMDR_MOCK_CONVENTIONS` dev run
  (which moves the webview alone: macOS keeps its bar and the page shows another desktop's beside it).
- **One table, five rows**: `conventions_on(Desktop)` answers macOS, Windows, and each Linux desktop in one `match`,
  compiled on every target; each OS arm (`macos.rs`, `windows.rs`, `linux/mod.rs`) only picks its row, Linux by the
  session's desktop (`apps/desktop/src-tauri/src/platform/linux/desktop.rs`, `XDG_CURRENT_DESKTOP` read once).
  `linuxDesktop` comes out of the row, so a desktop off Linux, or a GNOME row without GNOME, can't be built.
- Decision/Why: one cross-target table, unlike capabilities' answers inside each arm. Why: the browser demo and the test
  helpers need every desktop's row in TS, and a table compiled everywhere lets one Rust test pin `conventions-on.json`
  to it on any machine (it prints the JSON to write when they differ), where capabilities need a text-parsing test per
  arm file. Each target constructs only its own `Desktop`, hence the `dead_code` allows there.
- **The menu rows are D5's** (`docs/notes/windows-port/menu-model.md`): GNOME `primaryMenu`, Windows, KDE, and other
  Linux `windowMenuBar`; only Windows says `contextMenus: 'inPage'`, and only Windows
  `hiddenMenuBar: 'showsOnAltOrF10'`. A surface that draws a right-click menu asks `contextMenus`, ❌ never the
  platform.
- **Reading it**: `convention('dialogButtonOrder')` or `platformConventions()`. Each field reads the injected value only
  where it names a known member (the `every<…>()` lists in `conventions.ts` fail to compile when a Rust variant is added
  and not listed), else the fallback's, so a primitive never meets a value it has no branch for.
- **Tests**: `withConventions('kde')` sets a desktop's real row, `withConventions('gnome', { accessKeys: 'none' })` one
  field on top. It moves neither the platform nor the capabilities, the same split as `withPlatform()`.
- **What asks**: `ModalDialog` reads `dialogButtonOrder`, `enterActivates`, `cancelKeys`, and `accessKeys`
  (`../ui/DETAILS.md` § Platform conventions, through `../ui/dialog-keys.ts`), and so do the dialogs whose safe answer
  takes focus only where Return presses the focused button. The Windows theme (task 60), the menu model (63), and the
  screen reader pass (64) will read the rest.
- **An E2E run's right-click menus**: a `playwright-e2e` build reads `CMDR_E2E_CONTEXT_MENUS=inPage` (or `native`) and
  moves `contextMenus` alone, so the Linux lane drives Windows' in-page menus (`with_e2e_context_menus` in
  `src-tauri/src/platform/frontend.rs`; `../context-menu/DETAILS.md` § Running them off Windows). Every other build,
  release included, compiles it out.
- **Seeing another desktop's row**: `CMDR_MOCK_CONVENTIONS=windows pnpm dev` (or `gnome`, `kde`, `otherLinux`, `macos`;
  debug builds only, `mock_conventions` in `conventions.rs`) hands every webview that row instead, which moves neither
  the platform nor the capabilities. It also defines `__CMDR_COPY_PLATFORM__` (the mocked desktop's OS), which only
  `getCopyPlatform()` and so the message runtime's branch pick follow, so the window speaks that desktop's words too
  (`CMDR_MOCK_CONVENTIONS=gnome` on a Mac shows "Preferences"). The same value is `data-look` on `<html>` (`getLook()`),
  which keys the menu and control look tokens (`apps/desktop/src/DETAILS.md` § Menu look tokens and § Control look
  tokens), so the mock shows that desktop's menus and controls too (the browser demo's look follows its visitor the same
  way, `lib/demo/DETAILS.md` § Platform). **Decision/Why**: copy and conventions disagreeing in dev hid every desktop
  branch from a Mac; moving the platform itself would also move keymaps and capabilities. The native menu bar doesn't
  follow (`apps/desktop/src-tauri/src/intl/DETAILS.md`).

## The fallback

Contexts with no backend get `'linux'`, `NO_CAPABILITIES`, and `FALLBACK_CONVENTIONS` (the desktop-neutral Linux row, so
the two fallbacks agree):

- **Vitest**: the default test world is "not macOS, nothing offered". A test that needs another platform or a capability
  says so with `withPlatform()` / `withCapabilities()`.
- **A plain browser** (`vite dev` without Tauri): nothing answers IPC there anyway.
- **The browser demo** is the exception: its fake backend defines both globals itself from the visitor's user agent
  (`apps/desktop/src/lib/demo/platform.ts`), so every visitor gets their own OS's default keymap, as the keyboard under
  their hands expects, a Mac visitor gets the Mac app's surfaces, and everyone else gets only `appMenu` (the demo always
  draws the macOS bar, whose app menu owns Quit; `../demo/DETAILS.md` § Platform). A page has no other signal; the sniff
  lives in the demo's backend, never in app code.

Why not macOS as the fallback: off macOS is the safe direction. A Mac-only surface (Quick Look, Full Disk Access, the
Finder extension) hiding in an odd context is harmless; one showing where nothing can answer it isn't.

## Test helpers

`withPlatform()` and `withCapabilities()` set the same globals the init script does and return the undo, so everything
that reads them follows at once, `key-capture.ts`'s formatting included. Mocking `isMacOS` instead flipped only the
importers the mock reached and left the rest on the host's answer.

- Bare: the switch holds until the next one. Each test file starts on the fallback (Vitest isolates files).
- Per test: `beforeEach(() => withPlatform('macos'))` (Vitest runs the returned undo as the teardown).
- Inside one test: `onTestFinished(withCapabilities('fullDiskAccess'))`. `withCapabilities()` offers exactly what it
  names, nothing else.
- As data, for pure code that takes a set (`NudgeContext.capabilities`): `capabilitiesWith('dockPin')`.
- Before the imports, for module-load reads: `vi.hoisted(() => vi.stubGlobal('__CMDR_PLATFORM__', 'macos'))`, and
  `vi.stubGlobal('__CMDR_CAPABILITIES__', { quickLook: true })` (an absent field reads `false`). Hoisted code runs
  before any import, so it can't call the helpers and writes the globals itself. Decision: not
  `await vi.hoisted(async () => …import…)`, which reads better but breaks in a `.svelte.test.ts` (the Svelte transform
  nests the top-level `await`, and Vitest refuses a nested `vi.hoisted`; verified on Vitest 5.1.0, 2026-11-11).

Decision: `withPlatform('macos')` doesn't grant macOS's capabilities. Why: that would need a TS copy of each OS arm's
answers, which drifts from Rust, and a test about Full Disk Access should say it needs Full Disk Access rather than "a
Mac". A test of a Mac surface that also shows key glyphs sets both.

The one TS copy of the arms that does exist is `CAPABILITIES_POSSIBLE_ON`, behind `withCapabilitiesPossibleOn(os)`: what
a test of "what a user on that OS gets" needs (the default keymap snapshot in
`apps/desktop/src/lib/commands/default-keymap.test.ts`, conflicts, reachability). A field counts as possible unless its
arm answers a literal `false`, so a runtime answer (Linux's `cfg!(feature = "playwright-e2e")`, `x11_session()`) is in.
It can't drift: `capabilities-possible-on.test.ts` reads the three arm files as text and fails on any disagreement, a
field missing from either side included, and the arm files are in `svelteInputs` and the CI `svelte` filter so an arm
edit re-runs it. `withEveryCapability()` is for walking every surface, never for a per-OS test: it puts macOS-only
commands on Linux.

## Window chrome

`--titlebar-height` is the height of the overlay title bar: the main window's title bar sizes itself to it, and every
full-window backdrop starts below it so the window-drag region stays live under a dialog. Without one (the
`overlayTitleBar` capability, macOS only) the OS draws the title bar outside the webview, so `app.css` sets it to 0
under `:root:not([data-overlay-title-bar])` and a backdrop covers the whole window. The viewer's 35px override and the
queue window's in-webview title line follow the same capability.
