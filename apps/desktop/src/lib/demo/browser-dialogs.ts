/**
 * The dialog plugin's commands and the opener's folder reveal, answered as a page can: a question
 * (`plugin:dialog|message`, under `ask` and `confirm`) in the browser's own `confirm` / `alert`,
 * the OS's native alert of a web page. A file or folder picker and a reveal in the file manager
 * have no page equivalent over the demo's pretend files, so the visitor gets the demo's notice and
 * the app gets a picker's cancel. See `DETAILS.md` § Dialogs.
 */
import { addToast } from '#lib/ui/toast/index.js'
import { tString } from '#lib/intl/messages.svelte.js'

/** Long enough to read the two-sentence notice, as the archive one. */
const NOTICE_MS = 8000

/** The plugin's button set, as `@tauri-apps/plugin-dialog`'s `buttonsToRust` sends it. */
type Buttons =
  | 'Ok'
  | 'OkCancel'
  | 'YesNo'
  | 'YesNoCancel'
  | { OkCustom: string }
  | { OkCancelCustom: [string, string] }
  | { YesNoCancelCustom: [string, string, string] }

/**
 * The two answers a button set has, the confirming one first, as the plugin names the clicked
 * button; `null` for a lone OK. `YesNoCancel`'s middle button has no `confirm` equivalent: the
 * browser offers two, so No never comes back.
 */
function answersOf(buttons: Buttons | undefined): [string, string] | null {
  if (buttons === undefined || buttons === 'Ok') return null
  if (buttons === 'OkCancel') return ['Ok', 'Cancel']
  if (buttons === 'YesNo') return ['Yes', 'No']
  if (buttons === 'YesNoCancel') return ['Yes', 'Cancel']
  if ('OkCustom' in buttons) return null
  if ('OkCancelCustom' in buttons) return buttons.OkCancelCustom
  return [buttons.YesNoCancelCustom[0], buttons.YesNoCancelCustom[2]]
}

/** `plugin:dialog|message`: the clicked button's name, as the plugin answers. */
export function answerMessage(payload: Record<string, unknown>): string {
  const title = typeof payload.title === 'string' && payload.title !== '' ? `${payload.title}\n\n` : ''
  const text = title + String(payload.message)
  const buttons = payload.buttons as Buttons | undefined
  const answers = answersOf(buttons)
  if (answers === null) {
    window.alert(text)
    return typeof buttons === 'object' && 'OkCustom' in buttons ? buttons.OkCustom : 'Ok'
  }
  return window.confirm(text) ? answers[0] : answers[1]
}

/** Tells the visitor the installed app does this, and answers as a cancelled picker: `null`. */
function sayDesktopOnly(key: 'ui.demoDesktopOnly.filePicker' | 'ui.demoDesktopOnly.revealFolder'): null {
  addToast(tString(key), { level: 'info', timeoutMs: NOTICE_MS })
  return null
}

export const browserDialogAnswers: Partial<Record<string, (payload: Record<string, unknown>) => unknown>> = {
  'plugin:dialog|message': answerMessage,
  'plugin:dialog|open': () => sayDesktopOnly('ui.demoDesktopOnly.filePicker'),
  'plugin:dialog|save': () => sayDesktopOnly('ui.demoDesktopOnly.filePicker'),
  'plugin:opener|reveal_item_in_dir': () => sayDesktopOnly('ui.demoDesktopOnly.revealFolder'),
}
