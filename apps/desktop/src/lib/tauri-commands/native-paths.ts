// The frontend's one door from native path text to app text (#1311). A path the OS hands the
// webview (a drop's `paths`, a picker's answer) is the OS's own spelling, `C:\Users\…` on
// Windows, which is no app path; the backend crosses it (`commands/native_paths.rs`). On macOS
// and Linux the answer is the text sent.
//
// ❌ Never read `onDragDropEvent`'s `paths` or `@tauri-apps/plugin-dialog`'s `open` / `save`
// answer as an app path: `no-restricted-imports` in `eslint.config.js` sends new callers here.
// An OS-only path (a binary to run, a key file to read) may stay native, with a reasoned opt-out.

// eslint-disable-next-line no-restricted-imports -- the door itself: every drop crosses here.
import { getCurrentWebview } from '@tauri-apps/api/webview'
// eslint-disable-next-line no-restricted-imports -- the door itself: every app-path pick crosses here.
import { open, save, type OpenDialogOptions, type SaveDialogOptions } from '@tauri-apps/plugin-dialog'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { commands, events, type NativeDragDrop } from '#lib/ipc/bindings.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { getPlatform } from '#lib/platform/index.js'

const log = getAppLogger('nativePaths')

/** Native paths crossed to app text, in the order sent, and how many had no app path. */
interface AppPathsFromNative {
  paths: string[]
  /** Paths no app path names (a device namespace, a name needing an escape): left out. */
  refused: number
}

/** The app paths for native `paths` the OS handed the webview. */
async function appPathsFromNative(paths: string[]): Promise<AppPathsFromNative> {
  if (paths.length === 0) return { paths: [], refused: 0 }
  const answers = await commands.appPathsFromNative(paths)
  const crossed: string[] = []
  for (const answer of answers) {
    if (answer.kind === 'app') crossed.push(answer.path)
  }
  return { paths: crossed, refused: answers.length - crossed.length }
}

/** The app path for one native `path`, or `null` when no app path names it. */
async function appPathFromNative(path: string): Promise<string | null> {
  const { paths } = await appPathsFromNative([path])
  return paths[0] ?? null
}

/** A position in the webview, as the native drag event reports it. */
interface DragPosition {
  x: number
  y: number
}

/** A native drag-drop event with its `paths` already app paths. */
export type AppPathDragDropEvent =
  | { type: 'enter'; paths: string[]; refused: number; position: DragPosition }
  | { type: 'over'; position: DragPosition }
  | { type: 'drop'; paths: string[]; refused: number; position: DragPosition }
  | { type: 'leave' }

/**
 * A drag position in CSS pixels, the unit `elementsFromPoint` hit-tests in. macOS reports points,
 * which are CSS pixels already. Windows reports PHYSICAL pixels (wry's and Cmdr's own drop target
 * alike, both from `ScreenToClient`), so at 150 % display scaling a drop read them 1.5 times too
 * far right and down and hit the wrong row, pane, or nothing (#1311). `devicePixelRatio` folds
 * the display scale and the page zoom together.
 */
function toCssPixels(position: DragPosition): DragPosition {
  if (getPlatform() !== 'windows') return position
  const ratio = window.devicePixelRatio || 1
  return { x: position.x / ratio, y: position.y / ratio }
}

/**
 * Subscribes to the webview's native drag-drop events, each `enter` and `drop` delivered with
 * app paths and the position in CSS pixels. Events reach `handler` in the order the OS sent
 * them: an `over` waits for the `enter` before it to cross, so a drop target never reads a
 * drag's paths before they exist.
 *
 * Two sources, one queue: Tauri's own event (wry's drop target), and on Windows Cmdr's drop
 * target's `native-drag-drop` (`commands/file_system/drag/platform/windows/drop_target.rs`),
 * which replaces wry's there and sends paths already crossed. Only one of them speaks for a
 * window; listening to both keeps drops working if the replacement didn't take.
 */
export async function onAppPathDragDrop(handler: (event: AppPathDragDropEvent) => void): Promise<UnlistenFn> {
  let queue: Promise<void> = Promise.resolve()
  // A handler that throws is logged and the queue carries on: a rejected link would skip
  // every later event, so one bug would end drag-and-drop until a reload.
  const deliver = (type: AppPathDragDropEvent['type'], next: () => Promise<AppPathDragDropEvent>) => {
    queue = queue
      .then(async () => {
        handler(await next())
      })
      .catch((error: unknown) => {
        log.error('A drag-drop handler threw on a {type} event: {error}', { type, error })
      })
  }
  const webview = getCurrentWebview()
  // eslint-disable-next-line no-restricted-syntax -- the door itself: every drop crosses here.
  const fromTauri = await webview.onDragDropEvent((event) => {
    const payload = event.payload
    deliver(payload.type, async () => {
      if (payload.type === 'enter' || payload.type === 'drop') {
        const crossed = await crossOrRefuseAll(payload.paths)
        return { type: payload.type, ...crossed, position: toCssPixels(payload.position) }
      }
      if (payload.type === 'over') return { type: 'over', position: toCssPixels(payload.position) }
      return { type: 'leave' }
    })
  })
  if (getPlatform() !== 'windows') return fromTauri
  // Scoped to this webview: each window's drop target sends to its own.
  const fromCmdr = await events.nativeDragDrop(webview).listen((event) => {
    const payload = event.payload
    deliver(payload.type, () => Promise.resolve(fromCmdrDropTarget(payload)))
  })
  return () => {
    fromTauri()
    fromCmdr()
  }
}

/** A `native-drag-drop` event as the door hands it on: its paths are app paths already. */
function fromCmdrDropTarget(payload: NativeDragDrop): AppPathDragDropEvent {
  if (payload.type === 'enter' || payload.type === 'drop') {
    return {
      type: payload.type,
      paths: payload.paths,
      refused: payload.refused,
      position: toCssPixels(payload.position),
    }
  }
  if (payload.type === 'over') return { type: 'over', position: toCssPixels(payload.position) }
  return { type: 'leave' }
}

/** A broken bridge crosses nothing: every path counts as refused, so no native text slips by. */
async function crossOrRefuseAll(paths: string[]): Promise<AppPathsFromNative> {
  try {
    return await appPathsFromNative(paths)
  } catch {
    return { paths: [], refused: paths.length }
  }
}

/**
 * `@tauri-apps/plugin-dialog`'s `open` for ONE file or folder, answered as an app path: `null`
 * when the person cancelled, or when no app path names the pick.
 */
export async function pickOneAsAppPath(options: Omit<OpenDialogOptions, 'multiple'>): Promise<string | null> {
  const picked = await open({ ...options, multiple: false })
  return typeof picked === 'string' ? appPathFromNative(picked) : null
}

/** What the Save panel answered: cancelled, an app path, or a place no app path names. */
type SaveTarget = { kind: 'cancelled' } | { kind: 'chosen'; path: string } | { kind: 'refused' }

/** `@tauri-apps/plugin-dialog`'s `save`, answered as an app path. */
export async function chooseSaveTarget(options: SaveDialogOptions): Promise<SaveTarget> {
  const chosen = await save(options)
  if (chosen === null) return { kind: 'cancelled' }
  const path = await appPathFromNative(chosen)
  return path === null ? { kind: 'refused' } : { kind: 'chosen', path }
}
