/**
 * The pure half of the command menus (`MenuBar`, `ContextMenu`): reading `cmdr-menu`'s rows
 * (`Row`, `bindings.ts`), walking them, and giving them access keys. No DOM, no state. The rows
 * are the backend's (`crates/cmdr-menu/src/page/`), ❌ never decided here: this file only draws
 * and walks what came.
 */

import type { Row, RowLabel } from '#lib/ipc/bindings.js'
import type { MessageKey } from '#lib/intl/keys.gen.js'
import { getMessage } from '#lib/intl/messages.svelte.js'
import { accessKeySpanIn, allocateAccessKeys } from '../access-keys'

/**
 * How a command menu behaves, beyond its look (the look is `app-menu.css`'s tokens):
 * - `mac`: a macOS menu. ⌃F2 reaches the bar, no access keys, a radio choice wears the
 *   checkmark, shortcuts are spaced glyphs (`⇧⌘P`), and a submenu opens the moment it's hovered.
 * - `desktop`: a Windows or Linux menu. Alt or F10 reaches the bar, Alt plus a letter opens a
 *   menu, a letter picks a row, a radio choice wears a bullet, shortcuts are text
 *   (`Ctrl+Shift+P`), and a hovered submenu opens after the system's menu delay.
 */
export type MenuFlavor = 'mac' | 'desktop'

/** A row a person can act on: a command, a check, or a submenu. */
export type ActionRow = Extract<Row, { kind: 'item' | 'check' | 'submenu' }>

/** A row that picks: a command or a check. */
export type PickableRow = Extract<Row, { kind: 'item' | 'check' }>

/** A row's words: the catalog's, with each `{token}` replaced literally, as the desktop's `menu_t_with` does. */
export function rowLabelText(label: RowLabel): string {
  if (label.text !== undefined) return label.text
  let text = getMessage(label.key as MessageKey)
  for (const [token, value] of Object.entries(label.args)) text = text.split(`{${token}}`).join(value)
  return text
}

/** Whether `row` can be acted on now: a live command, check, or submenu. */
export function isLive(row: Row | undefined): row is ActionRow {
  return row !== undefined && (row.kind === 'item' || row.kind === 'check' || row.kind === 'submenu') && row.enabled
}

/** Whether `row` carries words a person reads as a row (everything but separators and the header line). */
function isLabeledRow(row: Row): row is Extract<Row, { kind: 'item' | 'check' | 'submenu' | 'system' }> {
  return row.kind === 'item' || row.kind === 'check' || row.kind === 'submenu' || row.kind === 'system'
}

/**
 * The next live row from `from`, `step` at a time, wrapping; `-1` when none is live. From `-1`
 * stepping forward lands on the first live row, and from `rows.length` stepping back on the last.
 */
export function nextLive(rows: readonly Row[], from: number, step: 1 | -1): number {
  const count = rows.length
  for (let i = 1; i <= count; i++) {
    const index = (((from + step * i) % count) + count) % count
    if (isLive(rows[index])) return index
  }
  return -1
}

/** The first or last live row, `-1` when none is. */
export function edgeLive(rows: readonly Row[], first: boolean): number {
  return first ? nextLive(rows, -1, 1) : nextLive(rows, rows.length, -1)
}

/**
 * Each row's access key, by index, allocated from the labels as shown, in the user's language
 * (`access-keys.ts`'s rule, the dialogs' and the Linux bar's): one per labeled row, greyed rows
 * included so a letter doesn't move when a row's state does. `null` for a separator, the header,
 * or a row the alphabet ran out for.
 */
export function rowAccessKeys(rows: readonly Row[], taken: Iterable<string> = []): (string | null)[] {
  const labeled = rows.flatMap((row, index) => (isLabeledRow(row) ? [index] : []))
  const letters = allocateAccessKeys(
    labeled.map((index) => {
      const row = rows[index]
      return isLabeledRow(row) ? rowLabelText(row.label) : ''
    }),
    taken,
  )
  const byIndex: (string | null)[] = rows.map(() => null)
  labeled.forEach((rowIndex, at) => {
    byIndex[rowIndex] = letters[at]
  })
  return byIndex
}

/** The label split around its access key, for drawing the underline; `suffix` is the "(B)" a label without its letter carries. */
export interface LabelParts {
  before: string
  key: string
  after: string
  suffix: string | null
}

export function labelParts(label: string, letter: string | null): LabelParts {
  if (letter === null) return { before: label, key: '', after: '', suffix: null }
  const span = accessKeySpanIn(label, letter)
  if (!span) return { before: label, key: '', after: '', suffix: letter.toUpperCase() }
  return {
    before: label.slice(0, span.start),
    key: label.slice(span.start, span.end),
    after: label.slice(span.end),
    suffix: null,
  }
}

/** A macOS shortcut's glyphs, one per modifier and one for the key, which is how a Mac spaces them. */
export function macShortcutGlyphs(shortcut: string): string[] {
  const glyphs: string[] = []
  let rest = shortcut
  while (rest.length > 1 && '⌃⌥⇧⌘'.includes(rest[0])) {
    glyphs.push(rest[0])
    rest = rest.slice(1)
  }
  glyphs.push(rest)
  return glyphs
}

/** The words of the header line a context menu leads with, which name the menu; `undefined` without one. */
export function headerText(rows: readonly Row[]): string | undefined {
  const header = rows.find((row) => row.kind === 'header')
  return header?.kind === 'header' ? rowLabelText(header.label) : undefined
}
