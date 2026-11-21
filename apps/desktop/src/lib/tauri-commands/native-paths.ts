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
import { commands } from '#lib/ipc/bindings.js'
import { getAppLogger } from '#lib/logging/logger.js'

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
 * Subscribes to the webview's native drag-drop events, each `enter` and `drop` delivered with
 * app paths. Events reach `handler` in the order the OS sent them: an `over` waits for the
 * `enter` before it to cross, so a drop target never reads a drag's paths before they exist.
 */
export async function onAppPathDragDrop(handler: (event: AppPathDragDropEvent) => void): Promise<UnlistenFn> {
  let queue: Promise<void> = Promise.resolve()
  // eslint-disable-next-line no-restricted-syntax -- the door itself: every drop crosses here.
  return getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload
    // A handler that throws is logged and the queue carries on: a rejected link would skip
    // every later event, so one bug would end drag-and-drop until a reload.
    queue = queue
      .then(async () => {
        if (payload.type === 'enter' || payload.type === 'drop') {
          const crossed = await crossOrRefuseAll(payload.paths)
          handler({ type: payload.type, ...crossed, position: payload.position })
        } else if (payload.type === 'over') {
          handler({ type: 'over', position: payload.position })
        } else {
          handler({ type: 'leave' })
        }
      })
      .catch((error: unknown) => {
        log.error('A drag-drop handler threw on a {type} event: {error}', { type: payload.type, error })
      })
  })
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
