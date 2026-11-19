/**
 * Fires a bar's row keys in a window that has no dispatcher of its own for them: the windows that
 * used to wear the main bar off macOS (Settings, the queue, Keyboard shortcuts, Debug), which now
 * draw no bar but keep every key it showed (Ctrl+W closes them, Ctrl+, opens Settings). Each key
 * picks its row through the caller's `pick`, the handler a native accelerator reached, so a
 * file-scoped row stays a no-op outside the main window, as it always was. Which key fires which
 * row: `bar-keys.ts`.
 *
 * ❗ Listens on `window` from the call on, so call it after the page's own keydown handlers are
 * attached (the root layout's `onMount` runs after the page's): a key the page took
 * (`defaultPrevented`) is never the bar's.
 */

import type { BarMenuView } from '#lib/ipc/bindings.js'
import { barRowForKey } from './bar-keys'

export interface WindowBarKeysDeps {
  /** Follows the bar: calls back with its rows now and after every change; resolves to the unsubscribe. */
  follow: (onMenus: (menus: BarMenuView[]) => void) => Promise<() => void>
  /** Picks a row by menu id, as a click on it would. */
  pick: (menuId: string) => void
}

/** Starts firing the followed bar's keys in this window; returns the teardown. */
export function fireWindowBarKeys(deps: WindowBarKeysDeps): () => void {
  let menus: readonly BarMenuView[] = []
  let stopFollowing: (() => void) | null = null
  let stopped = false
  void deps
    .follow((next) => {
      menus = next
    })
    .then((stop) => {
      if (stopped) stop()
      else stopFollowing = stop
    })
  const onKeyDown = (event: KeyboardEvent): void => {
    const id = barRowForKey(event, menus)
    if (id === null) return
    event.preventDefault()
    deps.pick(id)
  }
  window.addEventListener('keydown', onKeyDown)
  return () => {
    stopped = true
    stopFollowing?.()
    window.removeEventListener('keydown', onKeyDown)
  }
}
