/**
 * Every key a page-drawn bar shows FIRES (D5: off macOS nothing registers a bar key, so a shown
 * key that no page code runs would be a lie). The rows come from the backend's own bars, written by
 * `crates/cmdr-menu/src/page/bar_keys_test.rs` into `bar_keys.tsv`, so a new row or a moved key
 * reaches this test without anyone listing it here.
 *
 * - The viewer, and every window that used to wear the main bar (Settings, the queue, Keyboard
 *   shortcuts, Debug): the key picks its row through `barRowForKey`, the road `pick_menu_bar_item`
 *   runs a native accelerator's handler on.
 * - The main window: the key resolves to the row's own command in the window's dispatcher.
 * - The browser's own edit keys are the page's in every window (`LEFT_TO_THE_PAGE`, each with who
 *   runs it), as a native bar left them.
 */
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { BarMenuView, Row } from '#lib/ipc/bindings.js'
import type { Platform } from '#lib/platform/index.js'
import { withCapabilitiesPossibleOn, withPlatform } from '#lib/platform/test-platform.js'
import {
  destroyShortcutDispatch,
  initShortcutDispatch,
  lookupCommand,
  resolveKeyCombo,
} from '#lib/shortcuts/shortcut-dispatch.js'
import { barRowForKey, comboOfAriaShortcut } from './bar-keys'

const here = path.dirname(fileURLToPath(import.meta.url))
const FIXTURE = path.resolve(here, '../../../../../../crates/cmdr-menu/src/page/bar_keys.tsv')

interface ShownKey {
  platform: Platform
  window: 'main' | 'viewer'
  id: string
  aria: string
  command: string
}

function shownKeys(): ShownKey[] {
  return readFileSync(FIXTURE, 'utf8')
    .split('\n')
    .filter((line) => line !== '' && !line.startsWith('#'))
    .map((line) => {
      const [platform, window, id, aria, command] = line.split('\t')
      return { platform: platform as Platform, window: window as 'main' | 'viewer', id, aria, command }
    })
}

/** The rows whose keys the browser or the page's own handler runs, and who. */
const LEFT_TO_THE_PAGE: Record<string, string> = {
  edit_cut: 'a field cuts its own text',
  edit_copy: 'a field or the page selection copies its own text',
  edit_paste: 'a field pastes into itself',
  select_all_files: 'a field selects its own text; the queue selects its rows (`routes/queue/+page.svelte`)',
  viewer_edit_cut: 'the search box cuts its own text',
  viewer_edit_copy: "the viewer's keydown copies its own selection (`viewer-keyboard.ts`)",
  viewer_edit_paste: 'the search box pastes into itself',
  viewer_select_all: "the viewer's keydown selects the whole file (`viewer-keyboard.ts`)",
}

/** A US-layout character's physical key, and the character Shift types on it. */
const US_KEYS: Record<string, { code: string; shifted?: string }> = {
  ',': { code: 'Comma', shifted: '<' },
  '.': { code: 'Period', shifted: '>' },
  '=': { code: 'Equal', shifted: '+' },
  '-': { code: 'Minus', shifted: '_' },
  '\\': { code: 'Backslash', shifted: '|' },
  '*': { code: 'NumpadMultiply' },
  '+': { code: 'NumpadAdd' },
}

/** The keydown a person presses for `aria` on a US keyboard. */
function keydownFor(aria: string): KeyboardEvent {
  const parts = aria.split('+')
  const name = parts.pop() ?? ''
  const init: KeyboardEventInit = {
    bubbles: true,
    cancelable: true,
    ctrlKey: parts.includes('Control'),
    altKey: parts.includes('Alt'),
    shiftKey: parts.includes('Shift'),
    metaKey: parts.includes('Meta'),
  }
  const character = name === 'Plus' ? '+' : name
  if (/^[A-Z]$/.test(character))
    return new KeyboardEvent('keydown', { ...init, key: character.toLowerCase(), code: `Key${character}` })
  if (/^\d$/.test(character))
    return new KeyboardEvent('keydown', { ...init, key: character, code: `Digit${character}` })
  const us = US_KEYS[character] as { code: string; shifted?: string } | undefined
  if (us) {
    const key = init.shiftKey && us.shifted ? us.shifted : character
    return new KeyboardEvent('keydown', { ...init, key, code: us.code })
  }
  const named: Record<string, string> = { ArrowLeft: 'ArrowLeft', ArrowRight: 'ArrowRight', ArrowUp: 'ArrowUp' }
  return new KeyboardEvent('keydown', {
    ...init,
    key: named[character] ?? character,
    code: named[character] ?? character,
  })
}

/** The fixture's rows for one window as a one-menu bar, every row live. */
function barOf(keys: ShownKey[]): BarMenuView[] {
  const rows: Row[] = keys.map((key) => ({
    kind: 'item',
    id: key.id,
    label: { text: key.id },
    shortcut: key.aria,
    ariaKeyshortcuts: key.aria,
    enabled: true,
  }))
  return [{ id: 'menu', title: { text: 'Menu' }, rows }]
}

const ALL = shownKeys()

describe.each(['windows', 'linux'] as const)('every key the %s bars show', (platform) => {
  const undos: (() => void)[] = []
  beforeEach(() => {
    undos.push(withPlatform(platform), withCapabilitiesPossibleOn(platform))
  })
  afterEach(() => {
    destroyShortcutDispatch()
    for (const undo of undos.splice(0).reverse()) undo()
  })

  const keys = ALL.filter((key) => key.platform === platform)

  it('has rows to check', () => {
    expect(keys.some((key) => key.window === 'main')).toBe(true)
    expect(keys.some((key) => key.window === 'viewer')).toBe(true)
  })

  it.each(['main', 'viewer'] as const)(
    'picks its row in a window without a dispatcher of its own (%s bar)',
    (window) => {
      const rows = keys.filter((key) => key.window === window)
      const bar = barOf(rows)
      const dead: string[] = []
      for (const key of rows) {
        const picked = barRowForKey(keydownFor(key.aria), bar)
        if (key.id in LEFT_TO_THE_PAGE) {
          if (picked !== null) dead.push(`${key.id} (${key.aria}) is the page's, yet the bar took it`)
        } else if (picked !== key.id) {
          dead.push(`${key.id} (${key.aria}) picked ${picked ?? 'nothing'}`)
        }
      }
      expect(dead).toEqual([])
    },
  )

  it("runs the row's own command in the main window", () => {
    initShortcutDispatch()
    const dead: string[] = []
    for (const key of keys.filter((row) => row.window === 'main' && row.command !== '')) {
      const resolved = lookupCommand(resolveKeyCombo(keydownFor(key.aria)))
      if (resolved !== key.command)
        dead.push(`${key.id} (${key.aria}) runs ${resolved ?? 'nothing'}, not ${key.command}`)
    }
    expect(dead).toEqual([])
  })
})

describe('barRowForKey', () => {
  let undo: () => void
  beforeEach(() => {
    undo = withPlatform('linux')
  })
  afterEach(() => {
    undo()
  })

  const bar = barOf([
    { platform: 'linux', window: 'main', id: 'close_tab', aria: 'Control+W', command: '' },
    { platform: 'linux', window: 'main', id: 'file_copy', aria: 'F5', command: '' },
    { platform: 'linux', window: 'main', id: 'invert_selection', aria: '*', command: '' },
  ])

  it('leaves a key a page handler already took', () => {
    const event = keydownFor('Control+W')
    event.preventDefault()
    expect(barRowForKey(event, bar)).toBeNull()
  })

  it('leaves plain typing to a text field, and still fires a chord there', () => {
    const field = document.createElement('input')
    document.body.appendChild(field)
    const typed = new KeyboardEvent('keydown', { key: '*', code: 'NumpadMultiply', bubbles: true, cancelable: true })
    const chord = keydownFor('Control+W')
    let typedPick: string | null = 'unset'
    let chordPick: string | null = 'unset'
    field.addEventListener('keydown', (event) => {
      if (event === typed) typedPick = barRowForKey(event, bar)
      else chordPick = barRowForKey(event, bar)
    })
    field.dispatchEvent(typed)
    field.dispatchEvent(chord)
    field.remove()
    expect(typedPick).toBeNull()
    expect(chordPick).toBe('close_tab')
  })

  it('skips a greyed row', () => {
    const greyed = barOf([{ platform: 'linux', window: 'main', id: 'close_tab', aria: 'Control+W', command: '' }])
    const row = greyed[0].rows[0]
    if (row.kind === 'item') row.enabled = false
    expect(barRowForKey(keydownFor('Control+W'), greyed)).toBeNull()
  })

  it('spells aria shortcuts as canonical combos', () => {
    expect(comboOfAriaShortcut('Control+Shift+P')).toBe('Ctrl+Shift+P')
    expect(comboOfAriaShortcut('Alt+ArrowLeft')).toBe('Alt+←')
    expect(comboOfAriaShortcut('Control+Plus')).toBe('Ctrl++')
    expect(comboOfAriaShortcut('F5')).toBe('F5')
  })
})
