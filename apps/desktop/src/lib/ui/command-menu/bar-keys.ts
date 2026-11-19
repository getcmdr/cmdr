/**
 * A page-drawn bar's keys: which row a keypress fires, as a native bar's accelerator did.
 *
 * Off macOS no window wears a native bar (D5, task 63 of `docs/specs/windows-port-plan.md`), so a
 * key a row shows runs only if the page runs it. The main window's own dispatcher already does,
 * through the commands. The viewer and every window that used to wear the main bar (Settings, the
 * queue, Keyboard shortcuts, Debug) have no such dispatcher, so they ask this: the row whose shown
 * key (`ariaKeyshortcuts`) is the keypress, and the caller picks it through `pick_menu_bar_item`,
 * the handler a native accelerator reached. So a shown key is a key that fires, by construction.
 * `DETAILS.md` § Command menus has the rules; `bar-keys.test.ts` walks every row the backend shows.
 *
 * What it leaves alone, as a native bar did:
 * - A key a page handler already took (`defaultPrevented`): the page's own keys win.
 * - The browser's own edit keys (cut, copy, paste, select all, undo, redo): a field, the page's
 *   selection, or the page's own handler (the queue's Select all, the viewer's Copy) owns them.
 * - In a text field, the platform's native text keys (`native-text-keys.ts`) and plain typing.
 */

import type { BarMenuView, Row } from '#lib/ipc/bindings.js'
import { getPlatform, type Platform } from '#lib/platform/index.js'
import {
  formatChord,
  isAltGraphHeld,
  isModifierKey,
  isTypingKeyCombo,
  keyComboCandidates,
} from '#lib/shortcuts/key-capture.js'
import { nativeTextKeys } from '../../../routes/(main)/native-text-keys'

/** `aria-keyshortcuts` key names → the canonical key names `formatKeyCombo` writes. */
const ARIA_KEY_NAMES: Record<string, string> = {
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Plus: '+',
  Space: 'Space',
}

/** An `aria-keyshortcuts` value (`Control+Shift+P`) as the canonical combo a keypress formats to. */
export function comboOfAriaShortcut(aria: string): string | null {
  const parts = aria.split('+')
  const key = parts.pop()
  if (!key) return null
  const modifiers = { ctrlKey: false, altKey: false, shiftKey: false, metaKey: false }
  for (const part of parts) {
    if (part === 'Control') modifiers.ctrlKey = true
    else if (part === 'Alt') modifiers.altKey = true
    else if (part === 'Shift') modifiers.shiftKey = true
    else if (part === 'Meta') modifiers.metaKey = true
    else return null
  }
  const name = ARIA_KEY_NAMES[key] ?? (Array.from(key).length === 1 ? key.toUpperCase() : key)
  return formatChord(modifiers, name)
}

/** The browser's own text-edit keys on `platform`: a field or the page's selection owns them. */
function browserEditKeys(platform: Platform): ReadonlySet<string> {
  const primary = platform === 'macos' ? { metaKey: true } : { ctrlKey: true }
  const keys = ['X', 'C', 'V', 'A', 'Z', 'Y'].map((key) => formatChord(primary, key))
  keys.push(formatChord({ ...primary, shiftKey: true }, 'Z'))
  return new Set(keys)
}

function isTextField(target: EventTarget | null): boolean {
  if (target instanceof HTMLTextAreaElement) return true
  if (target instanceof HTMLInputElement)
    return !['checkbox', 'radio', 'range', 'button', 'submit'].includes(target.type)
  return target instanceof HTMLElement && target.isContentEditable
}

/** Whether `combo` is a key this keypress leaves to the page: an edit key, or a field's own key. */
function leftToThePage(combo: string, inField: boolean, platform: Platform): boolean {
  if (browserEditKeys(platform).has(combo)) return true
  if (!inField) return false
  const text = nativeTextKeys(platform)
  return (
    isTypingKeyCombo(combo) ||
    text.fieldOwned.has(combo) ||
    text.copyWithSelection.has(combo) ||
    text.bailInField.has(combo)
  )
}

/** Every pickable row with a key, submenus' included, as `[combo, row id]`. */
function keyedRows(rows: readonly Row[], into: Map<string, string>): void {
  for (const row of rows) {
    if (row.kind === 'submenu') {
      if (row.enabled) keyedRows(row.rows, into)
      continue
    }
    if (row.kind !== 'item' && row.kind !== 'check') continue
    if (!row.enabled || row.ariaKeyshortcuts === null) continue
    const combo = comboOfAriaShortcut(row.ariaKeyshortcuts)
    if (combo !== null && !into.has(combo)) into.set(combo, row.id)
  }
}

/**
 * The id of the row of `menus` whose shown key this keypress is, or `null` when it fires none.
 * The keypress's combos are tried most specific first (`keyComboCandidates`), as the main
 * window's dispatcher resolves them.
 */
export function barRowForKey(event: KeyboardEvent, menus: readonly BarMenuView[]): string | null {
  if (event.defaultPrevented || event.isComposing || isModifierKey(event.key) || isAltGraphHeld(event)) return null
  const byCombo = new Map<string, string>()
  for (const menu of menus) keyedRows(menu.rows, byCombo)
  const platform = getPlatform()
  const inField = isTextField(event.target)
  for (const combo of keyComboCandidates(event)) {
    if (leftToThePage(combo, inField, platform)) return null
    const id = byCombo.get(combo)
    if (id !== undefined) return id
  }
  return null
}
