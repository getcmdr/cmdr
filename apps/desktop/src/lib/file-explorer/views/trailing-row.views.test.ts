/**
 * Both views draw a kind's trailing action row (`trailing-row.ts`) from the generic prop alone:
 * last, after every entry (and after `..`'s offset), an option in the listbox that the cursor
 * reaches, that a click only lands the cursor on, that a double-click runs (never `onNavigate`),
 * and that's never a drop target or a drag source. The kind here is a stub: ❌ no view knows
 * which kind asked.
 */

import { describe, it, expect, vi } from 'vitest'
import { dirEntry, mountFullList } from './test-full-list'
import { mountBriefList } from './test-brief-list'
import type { TrailingRow } from './trailing-row'

vi.mock('#lib/tauri-commands/index.js', async () =>
  (await import('./test-file-list-mocks')).tauriCommandsMock({
    getBriefColumnTextWidths: () => Promise.resolve({ status: 'ok', data: { widths: [], missingCodePoints: [] } }),
  }),
)
vi.mock('#lib/icon-cache.js', async () => (await import('./test-file-list-mocks')).iconCacheMock())
vi.mock('#lib/indexing/index-state.svelte.js', async () => (await import('./test-file-list-mocks')).indexStateMock())
vi.mock('#lib/settings/settings-store.js', async () =>
  (await import('./test-file-list-mocks')).settingsStoreMock({ 'advanced.virtualizationBufferColumns': 2 }),
)
vi.mock('#lib/settings/reactive-settings.svelte.js', async () =>
  (await import('./test-file-list-mocks')).reactiveSettingsMock({
    getBriefColumnWidthMode: () => 'paneWidth',
    getBriefColumnWidthMaxPx: () => 400,
  }),
)

const STUB_ROW: TrailingRow = { label: 'Make a thing…', commandId: 'servers.connect', mcpName: '+ Make a thing…' }
const ENTRIES = [dirEntry({ name: 'alpha' }), dirEntry({ name: 'beta' })]

function trailing(target: HTMLElement): HTMLElement | null {
  return target.querySelector<HTMLElement>('[data-trailing-row]')
}

function options(target: HTMLElement): HTMLElement[] {
  return [...target.querySelectorAll<HTMLElement>('[role="option"]')]
}

describe.each([
  ['FullList', mountFullList],
  ['BriefList', mountBriefList],
] as const)('%s with a trailing row', (_name, mountList) => {
  it('draws it last, as an option the listbox counts, after the entries and `..`', async () => {
    const list = await mountList({
      entries: ENTRIES,
      props: { trailingRow: STUB_ROW, hasParent: true, parentPath: '/', totalCount: ENTRIES.length + 1 },
    })
    await list.settle(() => trailing(list.target) !== null, 'the trailing row')

    const all = options(list.target)
    const row = trailing(list.target)
    expect(all.at(-1)).toBe(row)
    expect(row?.getAttribute('aria-label')).toBe('Make a thing…')
    expect(row?.textContent).toContain('Make a thing…')
    // `..` + two entries + the trailing row: the trailing row is 4 of 4, and so is every row's set.
    expect(row?.getAttribute('aria-posinset')).toBe('4')
    expect(all.map((option) => option.getAttribute('aria-setsize'))).toEqual(['4', '4', '4', '4'])
    expect(row?.getAttribute('aria-selected')).toBe('false')
    // ❗ No drop target, no drag source.
    expect(row?.hasAttribute('data-drop-target-path')).toBe(false)
    expect(row?.getAttribute('draggable')).not.toBe('true')
  })

  it('wears the cursor at its index, which the listbox names as its active row', async () => {
    const list = await mountList({ entries: ENTRIES, props: { trailingRow: STUB_ROW, cursorIndex: ENTRIES.length } })
    await list.settle(() => trailing(list.target) !== null, 'the trailing row')
    const row = trailing(list.target)
    expect(row?.classList.contains('is-under-cursor')).toBe(true)
    list.component.focusList()
    const listbox = list.target.querySelector('[role="listbox"]')
    await list.settle(
      () => listbox?.getAttribute('aria-activedescendant') === row?.id,
      'the listbox to name the trailing row as its active row',
    )
  })

  it('a click (even a Shift- or ⌘-click) only lands the cursor; a double-click runs it, never onNavigate', async () => {
    const onSelect = vi.fn()
    const onNavigate = vi.fn()
    const onActivateTrailingRow = vi.fn()
    const list = await mountList({
      entries: ENTRIES,
      props: { trailingRow: STUB_ROW, onSelect, onNavigate, onActivateTrailingRow },
    })
    await list.settle(() => trailing(list.target) !== null, 'the trailing row')
    const row = trailing(list.target)
    row?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, shiftKey: true, metaKey: true }))
    expect(onSelect).toHaveBeenCalledWith({ index: ENTRIES.length })
    row?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
    expect(onActivateTrailingRow).toHaveBeenCalledTimes(1)
    expect(onNavigate).not.toHaveBeenCalled()
  })

  it('stays drawn on a list with no entries (a filter matching nothing), beside the empty state', async () => {
    const list = await mountList({ entries: [], props: { trailingRow: STUB_ROW, cursorIndex: 0 } })
    await list.settle(() => trailing(list.target) !== null, 'the trailing row')
    expect(trailing(list.target)?.getAttribute('aria-posinset')).toBe('1')
    expect(trailing(list.target)?.classList.contains('is-under-cursor')).toBe(true)
  })

  it('draws nothing extra when the kind asks for no trailing row', async () => {
    const list = await mountList({ entries: ENTRIES })
    expect(list.rowNames()).toEqual(['alpha', 'beta'])
    expect(trailing(list.target)).toBeNull()
    expect(options(list.target).map((option) => option.getAttribute('aria-setsize'))).toEqual(['2', '2'])
  })
})

describe('the views count the trailing row in their scroll extent and key ranges', () => {
  it("Full's spacer is one row taller", async () => {
    const list = await mountFullList({ entries: ENTRIES, props: { trailingRow: STUB_ROW } })
    const spacer = list.target.querySelector<HTMLElement>('.virtual-spacer')
    // Row height 20 (the settings mock): two entries plus the trailing row.
    expect(spacer?.style.height).toBe('60px')
  })

  it("Brief's ↓, →, and End reach it, and stop there", async () => {
    const list = await mountBriefList({
      entries: ENTRIES,
      props: { trailingRow: STUB_ROW, cursorIndex: ENTRIES.length - 1 },
    })
    const brief = list.component as unknown as {
      handleKeyNavigation: (key: string, event?: KeyboardEvent) => { newIndex: number; overflow: boolean } | undefined
    }
    expect(brief.handleKeyNavigation('ArrowDown')?.newIndex).toBe(ENTRIES.length)
    expect(brief.handleKeyNavigation('ArrowRight')?.newIndex).toBe(ENTRIES.length)
    expect(brief.handleKeyNavigation('End', new KeyboardEvent('keydown', { key: 'End' }))?.newIndex).toBe(
      ENTRIES.length,
    )
  })
})
