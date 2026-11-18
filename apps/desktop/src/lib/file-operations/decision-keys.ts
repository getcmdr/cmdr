/**
 * Total Commander-style letter answers for the file-operation prompts that ask
 * for a decision: the clash prompt (`transfer/TransferConflictDialog.svelte`,
 * inside the progress dialog and the main window's `OperationConflictDialog`)
 * and the transfer error dialog, plus the operation queue's row buttons, which
 * read their letters the same way. Every choice has ONE fixed letter, pressed
 * bare, and the button shows it in a chip (`Button`'s `keyHint`).
 *
 * The letters are not translated and never derived from a label: they're muscle
 * memory, so a German or Czech UI answers with the same keys as the English one.
 * A non-Latin layout reaches them through the physical key (`event.code`).
 *
 * The guards (whole combo, no repeat, no IME, no text field, a disabled button's
 * key does nothing) live here; the per-dialog arming delay lives with each
 * dialog. `DETAILS.md` § "Letter keys on decision prompts".
 */

import { claimKey } from '#lib/shortcuts/claim-key.js'
import { isTextInputTarget } from '#lib/utils/text-input-focus.js'
import { isButtonLike } from '#lib/ui/dialog-keys.js'

/**
 * How long a prompt has to be on screen before a key answers it. A clash or an
 * error can appear mid-keystroke (the main window raises itself for a
 * background clash), and a letter typed for something else must not pick
 * Overwrite. A new question can't be read and answered faster than this, so a
 * person answering on purpose never meets it.
 */
export const DECISION_KEYS_ARM_MS = 400

/** The clash prompt's keys: letters after Total Commander's copy dialog, plus Escape for Cancel. */
export const CONFLICT_KEYS = {
  skip: 'S',
  skipAll: 'K',
  rename: 'R',
  renameAll: 'N',
  overwrite: 'O',
  overwriteAll: 'A',
  overwriteAllSmaller: 'M',
  overwriteAllOlder: 'L',
  /** Escape, the key that stops things everywhere else. A key name, which is also ARIA's spelling and the chip's input. */
  cancel: 'Escape',
  rollback: 'B',
} as const

/** The transfer error dialog's letters. */
export const ERROR_KEYS = {
  retry: 'R',
  copyAnyway: 'A',
  close: 'C',
} as const

/**
 * The operation queue's row letters, acting on the row under the queue's cursor
 * (`queue/queue-keys.ts`). Rollback matches the clash prompt's, so a hand that
 * learned it there finds it here. Cancel stays C: Escape closes the queue
 * window, and a key that stops a copy can't also be the window's close. No
 * arming delay: nothing raises the queue mid-keystroke, and Rollback asks first
 * anyway.
 */
export const QUEUE_ROW_KEYS = {
  show: 'S',
  pauseOrResume: 'P',
  cancel: 'C',
  rollback: 'B',
} as const

/** One answer a prompt offers by key. */
export interface DecisionChoice {
  /** The fixed letter, uppercase. */
  key: string
  /** False whenever the button is disabled: the key does what a click would, which is nothing. */
  enabled: boolean
  run: () => void
}

/**
 * The bare key this keypress means to a decision prompt: an uppercase letter,
 * `'Enter'`, `'Escape'`, or `null` for a keypress that must not answer anything.
 *
 * - Any modifier means another combo (`⌘C` is copy, not Cancel), Shift included.
 * - A held key repeats, and a repeat must not answer the NEXT prompt too.
 * - An IME composition, or focus in a text field, means the person is typing.
 * - Enter on a focused button is never the prompt's default answer: where Return
 *   presses the focused button (Windows, Linux, a body button on the Mac) it
 *   belongs to that button, and on a Mac footer button Return does nothing
 *   (`ModalDialog`'s `handleReturn`), so it can't fall through to an answer.
 */
export function decisionKeyOf(event: KeyboardEvent): string | null {
  if (event.repeat || event.isComposing) return null
  if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return null
  if (isTextInputTarget(event.target)) return null
  if (event.key === 'Enter') return isButtonLike(event.target) ? null : 'Enter'
  if (event.key === 'Escape') return 'Escape'
  return letterOf(event)
}

/**
 * The Latin letter a keypress names: the typed one, or, for a single character
 * outside A-Z (a Cyrillic or Greek layout), the physical key's. `Process` and
 * other named keys (an IME's first keydown) aren't single characters, so they
 * never fall through to the physical key.
 */
function letterOf(event: KeyboardEvent): string | null {
  if (/^[a-z]$/i.test(event.key)) return event.key.toUpperCase()
  if (event.key.length !== 1) return null
  return /^Key([A-Z])$/.exec(event.code)?.[1] ?? null
}

/**
 * Answers a prompt from a keypress. `enterKey` names the choice Enter stands
 * for (the safe one). Returns whether a choice ran; a claimed key never reaches
 * the shortcut dispatcher.
 */
export function answerDecisionKey(event: KeyboardEvent, choices: readonly DecisionChoice[], enterKey: string): boolean {
  const key = decisionKeyOf(event)
  if (key === null) return false
  const wanted = key === 'Enter' ? enterKey : key
  const choice = choices.find((candidate) => candidate.key === wanted)
  if (choice === undefined || !choice.enabled) return false
  claimKey(event)
  choice.run()
  return true
}
