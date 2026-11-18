/**
 * The catalog a new window most likely speaks: the one the last language switch, in any window,
 * landed on. The message runtime loads it, and speaks it, before the window's first render, so a
 * language picked against the OS shows from the first frame instead of after the setting applies.
 *
 * Every Cmdr window shares one origin (`tauri://localhost`, or `http://tauri.localhost` in
 * WebView2), so one window's switch informs the next window and the next launch. A hint only: the `appearance.language` setting stays the truth, and
 * a stale hint (the macOS language changed while Cmdr was closed) costs one more catalog load when
 * the setting applies.
 *
 * Its own module, with no imports, so `test-setup.ts` can forget the hint BEFORE the runtime reads
 * it: Vitest keeps localStorage across files and runs.
 */

const STARTUP_CATALOG_KEY = 'cmdr.i18n.startupCatalog'

/** The remembered catalog tag, or `null` when there's none, it doesn't ship, or storage is off. */
export function rememberedStartupCatalog(shipped: readonly string[]): string | null {
  try {
    const tag = globalThis.localStorage.getItem(STARTUP_CATALOG_KEY)
    return tag !== null && shipped.includes(tag) ? tag : null
  } catch {
    return null
  }
}

/** Remembers the catalog a switch just landed on. */
export function rememberStartupCatalog(tag: string): void {
  try {
    globalThis.localStorage.setItem(STARTUP_CATALOG_KEY, tag)
  } catch {
    // Unwritable storage costs the next window one catalog load, nothing more.
  }
}

/** Test seam: forget the hint, so a fresh runtime starts on the webview's language. */
export function _forgetStartupCatalogForTests(): void {
  try {
    globalThis.localStorage.removeItem(STARTUP_CATALOG_KEY)
  } catch {
    // Nothing remembered, then.
  }
}
