# Details

Depth and rationale for this area. `CLAUDE.md` holds only the must-knows that prevent silent breakage; everything else
(architecture narrative, data flows, decision rationale, edge-case catalogs) lives here.

## Leading

One scale for the whole app: the four `--font-line-height-*` tokens, picked per surface in `docs/design-system.md` §
Leading. The text surfaces (`.modal-dialog`, `.toast`, the sheet, the secondary windows) INHERIT `normal` from
`app.css`, which is why a component usually writes no `line-height` at all. The main window and the file lists
deliberately inherit no ratio: their rows size from the density tiers, and a ratio there would fight them.

## Dev gates

A `.svelte` script gates dev-only behavior on `isDevBuild()` (`#lib/app-mode`), never on an inline
`import.meta.env.DEV`. knip collects a Svelte file's imports with a regex, and a bare `import.meta` matches its import
pattern: the match runs on to the next quoted string, the synthetic module knip parses comes out broken, and every
`await import(...)` in that file disappears from its graph. One `import.meta.env.DEV` in `routes/(main)/+page.svelte`
was enough to make knip call `lib/debug/debug-window.ts` an unused file while the app was opening the debug window from
it (knip 6.36.0, `dist/compilers/compilers.js` § `importMatcher`, verified 2026-09-21). A `.ts` file is parsed by
TypeScript and may write `import.meta.env.DEV` directly.

`{#if}` MARKUP may keep the literal: Vite replaces `import.meta.env.DEV` at build time, so the gated component leaves
the prod bundle. Its imports don't, though: every statically imported module stays in the graph, and one whose top level
calls a helper survives tree-shaking as a possible side effect (the dialog gallery's fixtures shipped this way, about 45
kB). So a dev-only SUBTREE loads through a dynamic import in a `.ts` loader whose branch folds to `null`, rendered with
`{#await}`: `lib/dialog-gallery/load-dialog-gallery.ts` and `routes/debug/load-debug-window.ts` (the whole Debug window)
are the two. A function call survives minification, so gating on `isDevBuild()` would ship the subtree to users either
way.

## Static assets

A `static/` file's URL is `asset('icons/x.svg')` from `$app/paths`, never `'/icons/x.svg'`. The app serves at the root
(base `''`), so both spellings resolve there, but the browser demo serves under `/demo` (`lib/demo/CLAUDE.md`), where a
root-relative path 404s: the Servers view's empty-state image did. `asset()` is typed against the files in `static/`, so
a typo or a missing file fails `svelte-check`; a template-built name needs a union-typed part
(`routes/dev/graphics/sections/StatusBadgesSection.svelte`). `cmdr/no-root-relative-asset` fails a literal whose first
segment is a top-level `static/` entry, in script, markup, and Svelte `<style>` `url()`, outside tests. Vitest has no
SvelteKit, so `$app/paths` resolves to `test-app-paths-shim.ts` (the root-base behavior).

## Glass material

Every frosted-glass surface (menus, selects, popovers, toasts, tooltips) draws from the tokens in `app.css` §
Frosted-glass material, and two per-window modules feed them the two macOS settings that shape it:

- **Reduced transparency.** WKWebView never reflects `@media (prefers-reduced-transparency)`, so the app can't key a
  fallback off it. The backend reads the `NSWorkspace` value and `#lib/display-accessibility` toggles an
  `html.reduce-transparency` CLASS instead (see § Display accessibility classes). Under it, `app.css` § Reduced
  transparency flips `--color-bg-glass` / `--color-bg-glass-steady` / `--color-border-glass` to opaque and
  `--glass-backdrop` to `none`, so a surface using the tokens needs no rule of its own. `prefers-reduced-motion`
  WKWebView does honor, so that one stays a media query.
- **Child windows' own material.** Settings, the queue, and Debug open see-through over `NSVisualEffectView` only where
  the `windowMaterial` capability says the OS draws one (macOS) and Reduce transparency is off; anywhere else they open
  opaque in the system appearance (`#lib/child-window-material`), and the page's translucent fill sits on that solid
  color. Gotcha/Why: the openers checked Reduce transparency alone, so on Windows Settings and the queue showed the
  windows behind them (verified on Windows 10 22H2 build 19045, WebView2 157.0.4341.60, by screenshot, 2026-11-19). The
  opaque fill is fixed at creation, so a live theme flip leaves it in the old appearance until the window reopens.
- **The Liquid Glass slider** (macOS 27 Appearance). The backend reads the undocumented `NSGlassTintAmount` global
  default, re-reading it each time the app becomes active
  (`apps/desktop/src-tauri/src/system_look/platform/macos/glass_tint.rs` has why no notification works), and
  `#lib/glass-material` sets `--glass-tint` (0 clearest to 1 most tinted, 0.5 when macOS reports none). The glass fill's
  opacity and the blur grow with it, as native menus do. Toasts ride it with a higher opacity floor
  (`--glass-toast-opacity`), since they hold paragraphs over busy lists; tooltips and the unblurred Ask Cmdr drop hint
  use the fixed `--color-bg-glass-steady` instead.

**Gotcha: the MCP bridge's `webview_screenshot` never paints `backdrop-filter`.** Every glass surface reads as bare
translucency in it, with the text underneath sharp through the fill, so a menu or popover over a busy list (a dialog's
preview list, say) looks broken while the real window blurs it fine. **Why it matters:** it reads like the nested
`backdrop-filter` trap (a filter whose ancestor has its own backdrop root sees only that ancestor's content), and an
agent will "fix" a surface that isn't broken. A menu inside a `ModalDialog` portals into the overlay, which carries no
`backdrop-filter` unless the dialog sets `blur`, and even then the panel sits inside that root, so the menu still blurs
the panel. Judge glass on a real display, never from that screenshot. (Verified in the dev app on macOS 27: a body-level
test div with `backdrop-filter: blur(20px)` above everything left the text under it pixel-sharp in the capture,
2026-10-09.)

Menu-like surfaces (`Menu`, `Select`, the breadcrumb popup) also take the macOS 26+ menu shape: `--radius-menu`,
`--shadow-glass` plus the `--shadow-glass-rim` top highlight, and rows highlighted as inset pills.

## Menu look tokens

`app-menu.css` (imported by `app.css`, like the palette) holds the look of the in-page menu bar, its flyouts, and the
context menus (task 63 of `docs/specs/windows-port-plan.md`): `--color-menu-*`, `--spacing-menu-*`, `--radius-menu-*`,
`--font-menu-*`, `--shadow-menu-surface`, and `--glass-menu-backdrop`. One token set keyed on the look, so the menu
component stays one implementation and reads only these.

- **Who reads them**: the command menus (`lib/ui/MenuBar.svelte`, `ContextMenu.svelte`, `lib/ui/command-menu/`; their
  DETAILS § Command menus), and only those. A caller with a look of its own sets the tokens on a class it hands the
  component: the browser demo's macOS 27 bar and menus (`lib/demo/menu-bar-style.ts`'s `DEMO_MENU_CLASS`).
- **`:root`** holds the house look (`lib/ui/Menu.svelte`'s row and surface, the demo bar's strip), the Linux in-page
  bar's until task 60 gives Linux its own neutral theme.
- **`:root[data-look='windows']`** is Windows 11's look, light and dark, on Windows 10 and 11 alike (D5). `data-look` is
  `getLook()` (`getCopyPlatform()`; the browser demo's is its visitor's OS), written by `applyPlatformAttributes()`
  beside `data-platform`, so `CMDR_MOCK_CONVENTIONS=windows pnpm dev` shows it on a Mac (without Segoe UI, which a Mac
  lacks).
- **Reduced transparency** (the `reduce-transparency` class, on under Windows' Transparency effects off or a contrast
  theme, § Display accessibility classes): acrylic's own opaque fallback color and no blur.
- **`forced-colors: active`** (Windows contrast themes): WinUI's `HighContrast` mapping as CSS system colors, no
  backdrop or shadow. Its selector list names the reduced-transparency selector too and comes later, so it wins over
  that block at equal weight. The bar and the surfaces read the controls' `--color-forced-adjust` (§ Control look
  tokens) into `forced-color-adjust`, or Chromium forces the highlighted row's `Highlight` back to `Canvas`; every other
  look keeps `auto`, so the browser forces the house look as it does any element.
- **Gotcha: the Safari 15.4 floor fails the build on `(forced-colors)` and `forced-color-adjust`**
  (`scripts/vite-webkit-css-floor.ts`), so both sit inside `@supports (forced-color-adjust: none)`, in the menu and the
  control tokens and in every component that sets the property. Every engine with contrast themes (Chromium, so
  WebView2, and Firefox) passes that guard; WebKit has no contrast themes to follow.
- **Gotcha: no `color-mix()` in the house defaults.** They're spelled out as `rgba()` of `--color-text-primary`, with a
  dark twin, because old WebKit drops a `color-mix()` value (`lib/utils/CLAUDE.md`).
- **Decision/Why: a file of its own, with hex allowed.** `app.css` is past its length budget, and the Windows values are
  WinUI's ARGB copied byte for byte, so `.stylelintrc.mjs` treats `app-menu.css` as a token file like `app.css`. Every
  selector carries its own specificity, so its import position doesn't matter.
- **The keyboard row** has tokens of its own (`--color-menu-row-bg-focus` / `-fg-focus`, the shortcut's and chevron's
  `-fg-focus`, `--color-menu-bar-item-bg-focus`): the house look fills it like the hover row, Windows gives it no fill
  and WinUI's two focus rings (`--color-menu-focus-*`), per `MenuFlyoutItem_Partial.cpp`; David's look check against
  Notepad confirms or reverses that.
- `app-menu.test.ts` pins the shape: every house token has a Windows value, every themed one a dark and a contrast value
  (system colors only), and the Windows text tokens clear AA on the fallback and on acrylic over black and white.

Each Windows value's WinUI resource key and file (at a pinned commit), and what CSS can't express (the luminosity blend,
noise, `ThemeShadow`, the font's optical size): `docs/notes/windows-port/menu-look-tokens.md`.

## Control look tokens

The `lib/ui` primitives' look (task 60): buttons, text fields (`TextInput`, `TextArea`, and the `Combobox` and
`NumberInput` frames), checkboxes, switches, radio buttons, sliders, progress bars, the select and the combobox list,
tooltips, `ModalDialog`'s chrome, focus rings, and scrollbars. Same shape as the menu tokens above, in three files that
`app.css` imports:

- **`app-controls.css`**: the house `:root` values, each one the literal its primitive wrote before, so macOS and Linux
  paint what they always did. `app-controls.test.ts` snapshots them.
- **`app-controls-windows.css`**: `:root[data-look='windows']`, WinUI's values in light and dark, and the Windows
  scrollbar rule (`:where()`-wrapped so a scroller that styles its own bar still wins). It also redefines the `app.css`
  tokens `ModalDialog` already reads (`--color-bg-dialog`, `--radius-dialog`, `--spacing-dialog`, `--shadow-dialog`, the
  dialog borders) and the field's `--radius-input` / `--color-field-border`. `data-look` is the menus' (`getLook()`), so
  a Windows visitor's browser demo wears these too.
- **`app-controls-windows-contrast.css`**: `forced-colors: active`, WinUI's `HighContrast` pairs as system colors. A
  primitive that paints them sets `forced-color-adjust: var(--color-forced-adjust)` (`none` only there), inside
  `@supports (forced-color-adjust: none)`, as the block itself is: the Safari 15.4 floor's guard (§ Menu look tokens).
- The select, the combobox list, and the tooltip read `app-menu.css`'s flyout surface on Windows (WinUI uses the same
  in-app acrylic for all three).
- **The accent fill**: every accent-filled Windows token reads `--color-accent-fill` / `--color-accent-fill-fg`
  (`app.css`: the accent and its contrast-picked text). Under the system accent, `#lib/accent-color` overrides both on
  `:root` with the shade the OS names for the current scheme (Windows' `AccentDark1` light, `AccentLight2` dark, from
  `SystemAccent.fill`) and WinUI's fixed white / black text, re-picked on a scheme flip with no IPC. macOS and Linux
  name no shades, so neither override is ever set there.
- **Decision/Why: disabled states re-point tokens.** A disabled control redeclares its state tokens as their `-disabled`
  twins on itself (`Checkbox`, `Switch`, `RadioGroup`, `Slider`), so the hover and press rules paint the disabled look
  with no specificity contest. The house twins equal the live values, because the house look dims with `opacity`
  instead.
- **Gotcha: `opacity` and `font-weight` can't be tokens.** Stylelint's allowlists for both take no `var()`, so the house
  dim (and the button weight) is keyed on the look in the selector, `:where(:root:not([data-look='windows'])) …`, which
  keeps the rule's specificity.
- **Gotcha: a house look with no border or edge** (the select trigger, the switch track) gets its Windows edge from an
  inset shadow or a whole `border` shorthand token (`none` in the house look), so the house computed style doesn't
  change at all.
- The a11y-contrast check (`scripts/check-a11y-contrast`) inlines `app.css`'s local `@import`s before it builds its var
  table, so it resolves these house tokens; it models the house look only, and the Windows contrast pairs are pinned by
  `app-controls.test.ts`.

Each Windows token's WinUI resource key and file, the elevation shadows, the computed-style proof that the house look
didn't move, and what CSS can't express: `docs/notes/windows-port/control-look-tokens.md`.

### Typography and motion

`app-type-motion-windows.css` (task 62, also imported by `app.css`) redefines `app.css`'s own `--font-system`,
`--font-size-lg`, and `--transition-*` under `:root[data-look='windows']`: Segoe UI Variable (its `Text` instance, then
Segoe UI for Windows 10), the WinUI type ramp's BodyLarge 18 for the house's 16px heading step, and WinUI's 83 ms
`BrushTransition` and 250 ms fast-out-slow-in curve. `html, body` read `--font-system`, so inherited text follows the
look too; `app.html` keeps the house stack as a literal for the boot screen. The menus' `--font-menu-family` reads the
same token. Windows' Animation effects setting arrives as `prefers-reduced-motion` through Chromium, so no backend
reader exists for it. `app-controls.test.ts` pins the house values and the Windows ones. Sources, the optical-size gap,
and the before/after proof: `docs/notes/windows-port/type-and-motion.md`.

## Display accessibility classes

`#lib/display-accessibility` (inited per window) mirrors four macOS Accessibility > Display options onto `<html>` as
classes: `reduce-transparency`, `increase-contrast`, `differentiate-without-color`, and `show-borders` (plus one Windows
setting, below). The backend reads all four from `NSWorkspace` in one typed `DisplayAccessibility` value and re-reads
them on every `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`
(`apps/desktop/src-tauri/src/system_look/platform/macos/display_accessibility.rs`), so they follow System Settings live.
The same value carries Invert colors, which gets no class: it only changes the icons macOS draws, which
`#lib/icon-cache` follows itself (`apps/desktop/src-tauri/src/icons/DETAILS.md` § Flushes and in-flight fetches).

- **Windows' Underline access keys** → `underline-access-keys`: the one Windows field (`underlineAccessKeys`, false on
  macOS and Linux), read with `SPI_GETKEYBOARDCUES`
  (`apps/desktop/src-tauri/src/system_look/platform/windows/keyboard_cues.rs`). With it on, access-key letters stay
  underlined all the time; task 63's step 4 has the menus' and dialogs' keyboard-cue rule read the class. Live: a hidden
  top-level window on its own thread hears every `WM_SETTINGCHANGE` broadcast, re-reads, and emits
  `display-accessibility-changed` only when a value moved
  (`apps/desktop/src-tauri/src/system_look/platform/windows/setting_changes.rs`; ❌ never a message-only window, which
  gets no broadcasts). That Settings' toggle broadcasts is unverified on a real machine (2026-11-18).
- **Windows' other two**: a contrast theme (`SPI_GETHIGHCONTRAST`) turns on `increase-contrast` AND
  `reduce-transparency` (WinUI drops acrylic under one), and Transparency effects off (`UISettings`'
  `AdvancedEffectsEnabled`) turns on `reduce-transparency`. The contrast theme's COLORS need no reader: WebView2 maps it
  to `forced-colors: active`, which the contrast token files style from. The same window re-reads both on
  `WM_SETTINGCHANGE`, `WM_THEMECHANGED`, `WM_SYSCOLORCHANGE`, and `UISettings`' change events.

- **Why classes**: WKWebView ignores `prefers-reduced-transparency` (verified, see the module doc), and CSS has no query
  for Differentiate without color or Show borders at all. `increase-contrast` is ALSO on when `prefers-contrast: more`
  matches, so a webview that reflects it (Linux) gets the same styling. Style off the class, ❌ never the media query:
  one source keeps the two from drifting.
- **Show borders has no public API**: the backend calls an undeclared `NSWorkspace` getter behind a
  `respondsToSelector:` guard (evidence in `display_accessibility.rs`). A standalone button that draws no edge of its
  own carries `.borderless-control` and gains a bezel under `html.show-borders` (`app-utilities.css` § Show borders).
  Give a new borderless standalone button the class, but ❌ not inline items (path segments, a tab's close button):
  macOS leaves those bare, and boxing each made the path bar read as broken.

## Window drag strips sit at `--z-sticky`, under every menu

Each secondary window (settings, debug, shortcuts, queue) paints an invisible `.window-drag-region` over its top strip
so the user can move the window from the empty space beside the traffic lights. It's absolutely positioned, so it
already paints over the in-flow layout — a rung is only needed to beat other POSITIONED chrome, and `--z-sticky` is as
high as it should ever go. All four sat at `--z-dropdown` once, which put an invisible strip on the same rung as the
app's menus and let it swallow clicks on their top rows: the AI provider pop-up in settings opens over its trigger, so
its first two options landed under the strip and couldn't be picked. Anything a menu can open into belongs BELOW
`--z-dropdown`. (A menu clears the strip only if its own rung really applies, which depends on the element carrying it:
`lib/ui/DETAILS.md` § Select.)

## Global stylesheets

Seven sheets, all global (no Svelte scoping). Who owns what:

- `app.css`: the design tokens (`:root`, plus the dark-mode, Increase contrast, reduced-transparency, and old-WebKit
  override blocks), then the base element styles (focus ring, typography layer, `html` / `body`, `#app-root`).
- `app-palette.css`: the static Tailwind color scale. Scheme-independent reference data with no `var()` dependencies, so
  it's order-independent.
- `app-reset.css`: the ress-derived reset, inside `@layer ress-reset`.
- `app-field.css`: the `.text-field*` chrome behind `lib/ui/TextInput.svelte` and `lib/ui/TextArea.svelte`. Nothing else
  may use those classes.
- `app-utilities.css`: class-per-value utilities applied via class bindings (`.size-*` size tiers, `.age-*` date ages,
  `.spinner*`), plus `.hit-target`, a small control's unpainted 24-by-24 hit box (WCAG 2.5.8).
- `app-tooltip.css`: `.cmdr-tooltip`, the singleton element the tooltip action creates.
- `app-file-list.css`: the `.file-entry` row chrome that `FullList.svelte` and `BriefList.svelte` share (stripe,
  selection fill, selected-row hairline, the marked-file bar and text stroke, cursor fill and outline, the unfocused
  pane's dashed cursor outline) and the lists' always-visible classic scrollbars. Only rules that were identical in both
  views live here; see `lib/file-explorer/views/DETAILS.md` for what stayed per-view and why. Also `.pane-footer-edge`,
  the top edge every pane footer shares (`lib/file-explorer/pane/DETAILS.md`, "One footer height everywhere").

### Cascade order is load order, and it's manual

`app.css` `@import`s the two order-independent sheets (palette, reset) at its top. The other four are imported from
`routes/+layout.svelte`, in a fixed order, AFTER `app.css`.

**Why:** a CSS `@import` must sit at the top of its sheet, so `@import`-ing a sheet whose rules belong at the END would
put them BEFORE everything that precedes them today. Wherever specificity ties, the winner flips, and the regression is
invisible until someone notices a wrong border weeks later. Importing from the layout is what reproduces the original
single-file order exactly. So: don't convert those four to `@import`s, and don't reorder the imports in the layout.

### Moving a rule out of a component costs a class of specificity

Svelte scopes a component rule by appending `.svelte-<hash>` to its FIRST compound selector (the rest get a
zero-specificity `:where(.svelte-<hash>)`). So lifting `.file-entry.is-selected` out of a `<style>` block drops it from
(0,3,0) to (0,2,0), and it starts tying with things it used to beat. `app-file-list.css` pays this back by prefixing
every selector with the view's container class (`.full-list-container` / `.brief-list-container`), which restores the
original specificity exactly. Without that, `DualPaneExplorer`'s `:global(.file-entry.folder-drop-target)` (also
(0,2,0), and emitted LATER: component styles ride the route chunk, these sheets ride the root-layout chunk) would win
the tie and paint the drag-over highlight over the selection and cursor fills.

Load order between the two chunks is real but should never be load-bearing: keep a lifted rule's specificity at or above
what it had inside the component.

The dark-mode / Increase contrast / reduced-transparency blocks in `app.css` are order-load-bearing for the same reason
(they override the light defaults above them). Leave them where they are.

**Verifying a move.** Vite's content hashes make this cheap: build the frontend before and after (`vite build` in
`apps/desktop`), then compare `build/_app/immutable/assets/*.css`. A move between two global sheets leaves every emitted
file byte-identical, hashed filenames included. A move OUT of a component rewrites selectors, so compare declarations
instead: extract every `(selector, property, value)` triple from the before and after bundles, normalize away the
`.svelte-<hash>` classes and the added container prefix, and diff the sets. Anything that shows up on one side only is a
rule you changed, not moved.

### Stylelint covers the global sheets too

`.stylelintrc.mjs` sets `customSyntax: 'postcss-html'` ONLY for `**/*.svelte` (it's what reaches a component's style
block). Set config-wide, it once parsed every plain `.css` as HTML, found no style block, and passed all seven sheets
silently: 300 findings had piled up behind it (#1056). `runStylelintCheck` probes for it every run (a known-bad `.css`
on stdin must report `declaration-no-important`), so the gap fails the lane instead of reopening quietly.

- The rules apply here as in components; `app.css` alone may write hex and `rgb()` (the `**/app.css` override), since
  it's where the tokens are defined.
- `--pane-tint-*-pct` sits in the `pane-` custom-property namespace: mix percentages, no color or spacing token.
- `app-reset.css` keeps the UA's `font-weight: bolder` / `700` (outside the 400/500/600 ladder) with a disable each:
  it's a reset, not a design decision.
- Gotcha: `stylelint --fix` escapes a literal `<style` inside a plain `.css` comment to `\3c style`. Write "style block"
  in comments.

## Error boundaries

A component that throws while it renders, runs an effect, or tears down must never blank a window. Five layers of
`<svelte:boundary>` make sure it can't, from the outside in:

- **Window**: `#lib/error-reporter/WindowBoundary.svelte`, wrapping everything in `routes/+layout.svelte`. Every window
  (main, Settings, viewer, queue, shortcuts, debug, the `/dev` pages) renders through that one layout, so one boundary
  covers them all. Its failed state is `PageCrashPanel` ("Something went wrong in this window", technical details, a
  report offer, Reload). Reload first re-arms in place with `reset()`, keeping the window's backend subscriptions; if
  the window breaks again within five seconds of that, the next Reload reloads the webview.
- **SvelteKit's per-route boundaries**: SvelteKit puts one around each route node below the root layout, and renders
  `routes/+error.svelte` (the same `PageCrashPanel`) when a page throws while rendering. They catch most page throws
  before the window boundary does; the window boundary takes the rest.
- **Pane**: `#lib/file-explorer/pane/PaneBoundary.svelte`, one per file pane in `DualPaneExplorer`'s `paneBlock`. A
  broken pane shows "This pane hit a problem" with Reload while the other pane, the tab bar, and every dialog keep
  working. Switching tabs in the broken pane re-arms it too.
- **Dialog**: `#lib/ui/boundary/DialogBoundary.svelte`. A broken dialog closes (through the host's `onClose`), a toast
  says so, focus goes home (`focusHome()`), and the boundary re-arms on the next tick, capped at three re-arms inside
  five seconds so a host whose close doesn't close can't spin. `DialogManager` puts ONE around every pane dialog, with
  an `onClose` that dismisses them all, because a stray `show*` flag suppresses the pane's keys
  (`lib/file-explorer/pane/DETAILS.md` § "A dialog that throws during render must never wedge input"). Every dialog a
  route mounts gets its own.
- **Rail**: the same `DialogBoundary`, around the Ask Cmdr rail's `{#if askCmdrState.open}` in the main route. A broken
  rail closes through `closeRail`, its toast names Ask Cmdr (the `closedMessage` prop), and the panes keep working.
  `lib/ask-cmdr/DETAILS.md` § "Layout, persistence, focus".

**Why a boundary has to sit ABOVE what it guards.** Svelte routes a throw from an effect's teardown to the nearest LIVE
ancestor boundary and skips one that is itself being destroyed (`execute_effect_teardown` → `invoke_error_boundary`,
verified on Svelte 5.57.2 by reading `internal/client/reactivity/effects.js` and `error-handling.js`, 2026-10-22). So a
dialog's boundary wraps the `{#if}` that mounts it, and the pane's wraps the per-tab `{#key}` that remounts it: a
boundary inside them is torn down along with what it guards, and the throw goes over its head to the window. That's how
a `derived_references_self` thrown while the Multi-rename sheet closed blanked the main window.

**The rule for new code.** A new window route needs nothing: the root layout covers it. A new dialog or side rail
mounted from a route goes in its own `<DialogBoundary name=… onClose=…>` around its `{#if}`;
`cmdr/dialog-inside-boundary` (`apps/desktop/eslint-plugins/dialog-inside-boundary.js`) fails the lint on one that isn't
(it knows a component by its name's suffix: `Dialog`, `Modal`, `Sheet`, `Rail`, and the rest the rule lists). `onClose`
clears whatever state mounts the dialog, usually its own close handler. Leave it out only for a dialog that owns its
visibility and starts closed when remounted (`StaleDriveDialog`). A dialog holding a paused operation's question
(`OperationConflictDialog`, `SourcesGonePrompt`, `LockedSourcesPrompt`) closes by cancelling that operation, since
nothing can answer it once its dialog is gone; the quit prompt calls the quit off.

**Reporting.** Each boundary's `onerror` logs the throw at error level, naming the boundary that caught it, so it lands
in the log file, error-report bundles, and an opted-in auto-send. SvelteKit also hands every boundary throw to
`hooks.client.ts`, which logs it with its frames and turns it into the `{ message, detail }` the boundary receives
(`lib/logging/DETAILS.md`). Boundaries never see throws in event handlers, timers, or promise callbacks: those reach the
global `app·uncaught` logger (`lib/logging/uncaught-errors.ts`) and leave the UI as it was.

Tests: `DialogBoundary.svelte.test.ts`, `PaneBoundary.svelte.test.ts`, `WindowBoundary.svelte.test.ts`, and
`lib/ask-cmdr/rail-boundary.svelte.test.ts` each mount a component that throws while rendering and one that throws while
tearing down (`test/fixtures/`), and assert the host survives and reports.
