/**
 * The dialog plugin's pickers and the opener's folder reveal, answered as a page can: they have no
 * page equivalent over the demo's pretend files, so the visitor gets the demo's notice and the app
 * gets a picker's cancel. A question (`plugin:dialog|message`) is the asking frame's own
 * (`question-dialog.ts`, through `own-window.ts`). See `DETAILS.md` § Dialogs.
 */
import { addToast } from '#lib/ui/toast/index.js'
import { tString } from '#lib/intl/messages.svelte.js'

/** Long enough to read the two-sentence notice, as the archive one. */
const NOTICE_MS = 8000

/** Tells the visitor the installed app does this, and answers as a cancelled picker: `null`. */
export function sayDesktopOnly(
  key: 'ui.demoDesktopOnly.filePicker' | 'ui.demoDesktopOnly.revealFolder' | 'ui.demoDesktopOnly.quit',
): null {
  addToast(tString(key), { level: 'info', timeoutMs: NOTICE_MS })
  return null
}

export const browserDialogAnswers: Partial<Record<string, (payload: Record<string, unknown>) => unknown>> = {
  'plugin:dialog|open': () => sayDesktopOnly('ui.demoDesktopOnly.filePicker'),
  'plugin:dialog|save': () => sayDesktopOnly('ui.demoDesktopOnly.filePicker'),
  'plugin:opener|reveal_item_in_dir': () => sayDesktopOnly('ui.demoDesktopOnly.revealFolder'),
}
