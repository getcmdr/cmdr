/**
 * axe over both views drawing a kind's trailing action row (`TrailingActionRow.svelte`), with
 * the cursor on it: an option in the listbox, named by its words, the listbox's active row.
 * Contrast is the a11y-contrast walker's, not jsdom's.
 */

import { describe, it, vi } from 'vitest'
import { expectNoA11yViolations } from '#lib/test-a11y.js'
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

describe.each([
  ['FullList', mountFullList],
  ['BriefList', mountBriefList],
] as const)('%s with a trailing row', (_name, mountList) => {
  it('has no a11y violations with the cursor on it', async () => {
    const list = await mountList({ entries: ENTRIES, props: { trailingRow: STUB_ROW, cursorIndex: ENTRIES.length } })
    await list.settle(() => list.target.querySelector('[data-trailing-row]') !== null, 'the trailing row')
    list.component.focusList()
    await expectNoA11yViolations(list.target)
  })

  it('has no a11y violations on a list with no entries but the row', async () => {
    const list = await mountList({ entries: [], props: { trailingRow: STUB_ROW, cursorIndex: 0 } })
    await list.settle(() => list.target.querySelector('[data-trailing-row]') !== null, 'the trailing row')
    await expectNoA11yViolations(list.target)
  })
})
