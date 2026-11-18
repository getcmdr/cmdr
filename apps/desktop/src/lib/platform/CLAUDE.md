# Platform (`lib/platform/`)

Which OS this window runs on (`'macos' | 'linux' | 'windows'`), what that OS lets Cmdr offer, and how its own apps
behave, as the backend says, read synchronously.

- `index.ts`: `getPlatform()`, `isMacOS()`, `hasCapability()`, `platformCapabilities()`, `applyPlatformAttributes()`.
- `conventions.ts`: `convention()`, `platformConventions()`, and `CONVENTIONS_ON` (`conventions-on.json`, every
  desktop's row, pinned by a Rust test).
- `test-platform.ts`: `withPlatform()`, `withCapabilities()`, `capabilitiesWith()`, `withCapabilitiesPossibleOn()`,
  `withConventions()`, the test helpers. A per-OS test (what a user on that OS gets) uses
  `withCapabilitiesPossibleOn(os)`, ❌ never `withEveryCapability()`; `capabilities-possible-on.test.ts` pins it to the
  Rust arms.

## Must-knows

- **To show or hide a feature, ask `hasCapability('quickLook')`, ❌ never `isMacOS()`.** The set is
  `PlatformCapabilities`, generated from Rust (`apps/desktop/src-tauri/src/platform/capabilities.rs`, one answer per OS
  arm). New OS-backed surface: add a field there, answer it in every arm, regen bindings. `isMacOS()` stays for
  genuinely per-platform answers: key glyphs, the OS version. ❌ Never to pick copy: per-OS wording is one catalog block
  (`../intl/DETAILS.md` § Platform variants).
- **To follow a platform's habit (dialog button order, Enter and Esc, access keys, menu model, screen readers), ask
  `convention('dialogButtonOrder')`, ❌ never the platform.** Linux answers per desktop (GNOME, KDE, other), so a
  primitive that asks gets every desktop right with no branch. Source: `conventions_on` in
  `apps/desktop/src-tauri/src/platform/conventions.rs`; guideline evidence:
  `docs/notes/windows-port/platform-conventions.md`.
- **All three values are injected, never sniffed.** The backend's init script defines `window.__CMDR_PLATFORM__`,
  `__CMDR_CAPABILITIES__`, and `__CMDR_CONVENTIONS__` in every webview before any script runs, so module-load reads are
  safe. ❌ Never read `navigator.userAgent` or `navigator.platform` to decide the OS.
- **No backend, no injection: `'linux'`, no capabilities, and the desktop-neutral Linux conventions** (Vitest, a plain
  browser), so no OS-backed surface shows by accident. The browser demo answers for the visitor's computer
  (`apps/desktop/src/lib/demo/platform.ts`).
- **The three move independently in tests.** `withPlatform()` moves the platform (key formatting follows),
  `withCapabilities('fullDiskAccess')` what the OS offers, `withConventions('kde')` how it behaves; ❌ never mock
  `isMacOS` / `hasCapability` or stub the user agent. Module-load reads need `vi.hoisted`; the forms are in
  `test-platform.ts`.
- **CSS follows `data-overlay-title-bar`** (and `data-platform`), set by `hooks.client.ts` before the first route
  renders. `--titlebar-height` is 0 without an overlay title bar (`app.css` § Window chrome).

Mechanics, the fallback, and decisions: `DETAILS.md`.
