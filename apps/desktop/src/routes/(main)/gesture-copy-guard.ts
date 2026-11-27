/**
 * Ignores a file-scope ⌘C that lands right after a mouse gesture.
 *
 * Selection utilities (PopClip-style) post a synthetic ⌘C right after a double-click or a drag,
 * to grab the text the gesture selected. In Cmdr that keystroke reached `edit.copy` with no text
 * selected, so it put the cursor file on the clipboard (silently replacing what was there) and
 * toasted "1 file copied" on every double-click on `..` and every window drag (ERR-YTH9K,
 * ERR-7KEBV: the copy landed 1–115 ms after each gesture, with no key press).
 *
 * It's a timing guard on purpose. The synthetic ⌘C takes the same road as a real one (the
 * native Edit > Copy accelerator), and by the time the menu event reaches Rust,
 * `NSApp.currentEvent` can already be a later event, so the CGEvent source can't reliably
 * tell the two apart there. Only the FILE copy is guarded: the `edit.copy` handler copies a
 * focused field's or a text selection's text first, so a real text selection still copies.
 *
 * Gestures, both seen from the main webview:
 * - `doubleClick`: a left-button `mouseup` with a click count of 2 or more (a triple-click too).
 * - `windowDrag`: the window moved. The title bar is a `data-tauri-drag-region`, so AppKit runs
 *   the drag and the webview never sees its `mouseup`; the move events mark it instead. A
 *   double-click on the title bar zooms the window, which moves it too.
 */
import { getAppLogger } from '#lib/logging/logger.js'
import type { DispatchSource } from './command-dispatch-context'

const log = getAppLogger('user-action')

export type MouseGesture = 'doubleClick' | 'windowDrag'

/**
 * How long after a gesture a file copy is ignored. The utility's ⌘C arrived within 115 ms in the
 * reports; half a second covers a slow main thread, and a person rarely double-clicks or drags
 * the window and then copies files inside it. David accepted that trade-off (cmdr-reports#126).
 */
export const GESTURE_COPY_GUARD_MS = 500

/** The roads a keystroke, real or synthetic, can take. Every other road is a deliberate pick. */
const KEYSTROKE_SOURCES: ReadonlySet<DispatchSource> = new Set<DispatchSource>(['keyboard', 'menu'])

let lastGesture: { gesture: MouseGesture; at: number } | null = null

/** Records a gesture. `now` is injectable for tests. */
export function noteMouseGesture(gesture: MouseGesture, now: number = Date.now()): void {
  lastGesture = { gesture, at: now }
}

/**
 * Whether a file copy from `source` should be skipped because it arrived right after a gesture.
 * Logs at debug when it does, so a report shows the swallowed copy.
 */
export function shouldIgnoreFileCopy(source: DispatchSource, now: number = Date.now()): boolean {
  if (!KEYSTROKE_SOURCES.has(source) || lastGesture === null) return false
  const ageMs = now - lastGesture.at
  if (ageMs >= GESTURE_COPY_GUARD_MS) return false
  log.debug("Ignored a file copy from the {source} {ageMs} ms after a {gesture}: likely a selection utility's ⌘C", {
    source,
    ageMs,
    gesture: lastGesture.gesture,
  })
  return true
}

export interface GestureCopyGuardDeps {
  /** Where mouse events land: the main window. */
  target: EventTarget
  /** Subscribes to the window's move events (Tauri's `onMoved`). May reject outside Tauri. */
  onWindowMoved: (onMoved: () => void) => Promise<() => void>
}

/** Starts noting gestures. Resolves to the teardown. */
export async function startGestureCopyGuard({ target, onWindowMoved }: GestureCopyGuardDeps): Promise<() => void> {
  const onMouseUp = (event: Event): void => {
    if (event instanceof MouseEvent && event.button === 0 && event.detail >= 2) noteMouseGesture('doubleClick')
  }
  // Capture phase: a pane handler that stops propagation can't hide the gesture.
  target.addEventListener('mouseup', onMouseUp, { capture: true })

  let unlistenMoved: (() => void) | null = null
  try {
    unlistenMoved = await onWindowMoved(() => {
      noteMouseGesture('windowDrag')
    })
  } catch {
    // Not in Tauri (Playwright smoke tests): double-clicks are still guarded.
  }

  return () => {
    target.removeEventListener('mouseup', onMouseUp, { capture: true })
    unlistenMoved?.()
  }
}

/** Test-only: forget the last gesture. */
export function _resetGestureCopyGuardForTests(): void {
  lastGesture = null
}
