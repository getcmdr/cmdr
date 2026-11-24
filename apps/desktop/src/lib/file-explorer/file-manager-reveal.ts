/**
 * "Show in Finder / File Explorer / file manager" (the shortcut, the File menu, the context
 * menu, the About card): ask the OS's file manager to show the item, or say plainly why it
 * didn't. Every refusal ends the same way, a toast, whether it came before anything was asked
 * (no path the file manager takes) or from the file manager itself (the item is gone).
 * The words live here; the reasons are typed in Rust (`commands/file_actions/platform/mod.rs`).
 */

import { showInFinder, type RevealError } from '#lib/tauri-commands/index.js'
import { addToast } from '#lib/ui/toast/index.js'
import { tString } from '#lib/intl/messages.svelte.js'
import { getAppLogger } from '#lib/logging/logger.js'
import type { MessageKey } from '#lib/intl/keys.gen.js'

const log = getAppLogger('fileExplorer')

/** Long enough to read a two-sentence notice. */
const NOTICE_TIMEOUT_MS = 8000

const MESSAGE_FOR: Record<RevealError['type'], MessageKey> = {
  noPath: 'fileExplorer.reveal.noPath',
  noPlainPath: 'fileExplorer.reveal.noPlainPath',
  notShown: 'fileExplorer.reveal.notShown',
}

/**
 * Shows `path` in the OS's file manager, or raises a toast saying why it didn't. Never throws:
 * the callers are fire-and-forget key, menu, and button handlers.
 * @returns Whether the file manager was asked and didn't refuse.
 */
export async function showInFileManagerOrExplain(path: string): Promise<boolean> {
  let refusal: RevealError | null
  try {
    refusal = await showInFinder(path)
  } catch (error) {
    log.warn('The reveal command threw: {error}', { error })
    refusal = { type: 'notShown' }
  }
  if (refusal === null) return true
  log.info('Not shown in the file manager: {reason}', { reason: refusal.type })
  addToast(tString(MESSAGE_FOR[refusal.type]), { level: 'warn', timeoutMs: NOTICE_TIMEOUT_MS })
  return false
}
