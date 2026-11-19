import { type UnlistenFn } from '@tauri-apps/api/event'
import { commands, events } from '#lib/ipc/bindings.js'
import { throwIpcError } from './ipc-types'
import { popContextMenu } from './in-page-menus'

/** Shows the function key bar's context menu, native or in-page by the platform (`popContextMenu`). */
export async function showFunctionKeyBarContextMenu(): Promise<void> {
  await popContextMenu({ menu: 'functionKeyBar' }, async () => {
    const res = await commands.showFunctionKeyBarContextMenu()
    if (res.status === 'error') throwIpcError(res.error)
  })
}

/**
 * Listens for the "Hide function key bar" context-menu item click, emitted by
 * `on_menu_event` in Rust. No payload: the frontend owns both the setting write
 * and the confirmation toast.
 */
export function onFunctionKeyBarHideRequested(handler: () => void): Promise<UnlistenFn> {
  return events.functionKeyBarHideRequested.listen(() => {
    handler()
  })
}
