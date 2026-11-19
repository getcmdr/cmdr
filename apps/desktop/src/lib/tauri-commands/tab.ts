import { type UnlistenFn } from '@tauri-apps/api/event'
import { commands, events } from '#lib/ipc/bindings.js'
import { throwIpcError } from './ipc-types'
import { popContextMenu } from './in-page-menus'

/** Shows a tab's context menu, native or in-page by the platform (`popContextMenu`); resolves once it's closed. */
export async function showTabContextMenu(
  isPinned: boolean,
  canClose: boolean,
  hasOtherUnpinnedTabs: boolean,
): Promise<void> {
  await popContextMenu({ menu: 'tab', isPinned, canClose, hasOtherUnpinnedTabs }, async () => {
    const res = await commands.showTabContextMenu(isPinned, canClose, hasOtherUnpinnedTabs)
    if (res.status === 'error') throwIpcError(res.error)
  })
}

/**
 * Listens for the tab context menu action event emitted by `on_menu_event` in Rust.
 * The event fires asynchronously after `popup()` returns because muda queues the
 * `MenuEvent` through the event loop.
 */
export function onTabContextAction(handler: (action: string) => void): Promise<UnlistenFn> {
  return events.tabContextAction.listen((event) => {
    handler(event.payload.action)
  })
}
