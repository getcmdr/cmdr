/**
 * Debug window management.
 *
 * Mirrors `settings-window.ts`'s chrome (traffic lights, vibrancy, rounded
 * corners) so the debug window feels like a first-class child window even
 * though it's dev-only. **No production callers depend on this** — Debug
 * is throwaway by design.
 *
 * Why duplicate `settings-window.ts`?
 * - Different size budget: Debug doesn't scale with text size, and its
 *   sidebar items are wider section names ("SMB diagnostics", "Toast
 *   notifications") that need a fixed-width sidebar.
 * - Clean teardown: removing the debug window later means deleting one
 *   file; no shared-helper webs to untangle.
 */

import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { LogicalPosition } from '@tauri-apps/api/dpi'
import { Effect, EffectState } from '@tauri-apps/api/window'
import { raiseChildWindow } from '#lib/tauri-commands/index.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { decorateChildWindowTitle } from '#lib/app-mode.js'
import { readMainRect, readMonitors, readSavedRect, resolveChildPosition } from '#lib/window-positioning.js'
import { scheduleShowFallback } from '#lib/child-window-show.js'
import { childWindowMaterial } from '#lib/child-window-material.js'

const log = getAppLogger('debug')

/** Sidebar (260 px) + content padding (32 px) = 292 px of fixed chrome. */
export const DEBUG_WIDTH = 920
export const DEBUG_MIN_WIDTH = 720
export const DEBUG_HEIGHT = 720
export const DEBUG_MIN_HEIGHT = 480

/**
 * Opens the debug window, or focuses it if already open. Dev-only —
 * the caller in `routes/(main)/+page.svelte` only invokes this under
 * `import.meta.env.DEV`.
 */
export async function openDebugWindow(): Promise<void> {
  const existing = await WebviewWindow.getByLabel('debug')
  if (existing) {
    // Not `setFocus()`: it does nothing to a minimized or hidden window, and the backend also
    // brings back a window caught mid-close.
    await raiseChildWindow('debug')
    return
  }

  log.debug('Creating new debug window')

  const [main, monitors, saved] = await Promise.all([readMainRect(), readMonitors(), readSavedRect('debug')])
  const rect = main
    ? resolveChildPosition({ size: { width: DEBUG_WIDTH, height: DEBUG_HEIGHT }, main, monitors, saved })
    : null

  // See-through over the macOS material, or opaque, as Settings opens (`#lib/child-window-material`).
  const { glass, backgroundColor } = await childWindowMaterial()

  const win = new WebviewWindow('debug', {
    url: '/debug',
    title: decorateChildWindowTitle('Debug'),
    width: DEBUG_WIDTH,
    height: DEBUG_HEIGHT,
    minWidth: DEBUG_MIN_WIDTH,
    minHeight: DEBUG_MIN_HEIGHT,
    ...(rect ? { x: rect.x, y: rect.y } : { center: true }),
    resizable: true,
    decorations: true,
    focus: true,
    // Created hidden; the debug page shows it, sliding in, once it's ready
    // (`#lib/child-window-show`).
    visible: false,
    // Translucent glass backdrop, mirroring Settings, where `glass` says so. Requires `tauri/macos-private-api` (already
    // enabled for Settings).
    transparent: glass,
    backgroundColor,
    // Overlay title bar so traffic lights can sit inside the sidebar.
    titleBarStyle: 'overlay',
    hiddenTitle: true,
    trafficLightPosition: new LogicalPosition(20, 29),
  })
  scheduleShowFallback('debug')

  // Apply NSVisualEffectView Sidebar material after creation — only when
  // transparency is allowed. Same reasoning as `settings-window.ts`:
  // `windowEffects` in the options dict was dropped on the way to the Rust
  // runtime in this Tauri version, so the explicit IPC path is the reliable
  // one. Radius matches `--radius-window` in `app.css`.
  if (glass) {
    void win.once('tauri://created', () => {
      void win
        .setEffects({
          effects: [Effect.Sidebar],
          state: EffectState.FollowsWindowActiveState,
          radius: 16,
        })
        .catch((error: unknown) => {
          log.debug('Failed to apply debug window effects: {error}', { error })
        })
    })
  }
}
