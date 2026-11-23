import { setMainWindowVisible } from '#lib/tauri-commands/index.js'

/**
 * Reports the main window's visibility to the backend: once now, then on every `visibilitychange`.
 *
 * WebKit flips `document.visibilityState` for every way the window stops being seen (minimized, app
 * hidden, another Space, fully covered), so this is the backend's one source for "is anyone looking"
 * (`src-tauri/src/main_window_visibility.rs`). Returns the teardown.
 */
export function startMainWindowVisibilityReport(): () => void {
  const report = (): void => {
    // Fire-and-forget: a lost report leaves the backend on its last answer, which the next change fixes.
    setMainWindowVisible(isMainWindowVisible()).catch(() => {})
  }
  report()
  document.addEventListener('visibilitychange', report)
  return () => {
    document.removeEventListener('visibilitychange', report)
  }
}

/**
 * Whether anyone can see the main window right now, the same answer the backend gets. ❗ Visible but
 * not frontmost (another app focused, Cmdr still on screen) is `true`: a person can still see it.
 */
export function isMainWindowVisible(): boolean {
  return document.visibilityState === 'visible'
}

/** Calls `onShown` each time the main window becomes visible again. Returns the teardown. */
export function onMainWindowShown(onShown: () => void): () => void {
  const handle = (): void => {
    if (isMainWindowVisible()) onShown()
  }
  document.addEventListener('visibilitychange', handle)
  return () => {
    document.removeEventListener('visibilitychange', handle)
  }
}
