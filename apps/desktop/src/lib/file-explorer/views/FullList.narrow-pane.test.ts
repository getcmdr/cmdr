/**
 * A narrow Full pane drops its date column first, then Ext, so the Name column
 * never collapses to 0 px (the 950 px window with a 25 % split left ~240 px and
 * the fixed tracks ate all of it). Header cells drop with their columns, a
 * dropped Ext folds the extension back into Name (as `showExtensionInName`
 * does, keeping the Ext sort trigger), and a wide pane renders exactly as before.
 *
 * Widths come from a fake measurer (7 px per character), so the fixed tracks are
 * realistic: Ext 28, Size 40, date 107 px at 100 % text size.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { dirEntry, fileEntry, mountFullList } from './test-full-list'
import { _setMeasureForTests } from './measure-column-widths'

vi.mock('#lib/tauri-commands/index.js', async () => (await import('./test-file-list-mocks')).tauriCommandsMock())
vi.mock('#lib/icon-cache.js', async () => (await import('./test-file-list-mocks')).iconCacheMock())
vi.mock('#lib/indexing/index-state.svelte.js', async () => (await import('./test-file-list-mocks')).indexStateMock())
vi.mock('#lib/settings/settings-store.js', async () => (await import('./test-file-list-mocks')).settingsStoreMock())
vi.mock('#lib/settings/reactive-settings.svelte.js', async () =>
  (await import('./test-file-list-mocks')).reactiveSettingsMock(),
)

beforeEach(() => {
  _setMeasureForTests((text) => text.length * 7)
})
afterEach(() => {
  _setMeasureForTests(null)
})

const mountAt = (clientWidth: number) =>
  mountFullList({
    entries: [fileEntry({ name: 'report.md', iconId: 'ext:md', size: 2048 })],
    viewport: { clientHeight: 400, clientWidth, offsetWidth: clientWidth },
  })

function headerSortLabels(target: HTMLElement): string[] {
  return [...target.querySelectorAll('.header-row .sortable-header .label')].map((l) => l.textContent)
}

function trackCount(gridTemplate: string): number {
  return gridTemplate.trim().split(/\s+/).length
}

function rowTemplate(target: HTMLElement): string {
  return target.querySelector<HTMLElement>('.file-entry')?.style.gridTemplateColumns ?? ''
}

describe('FullList in a narrow pane', () => {
  it('keeps every column in a wide pane (layout unchanged)', async () => {
    const list = await mountAt(800)
    expect(list.rowNames()).toEqual(['report.md'])
    expect(list.target.querySelectorAll('.col-ext').length).toBe(1)
    expect(list.target.querySelectorAll('.col-date').length).toBe(1)
    expect(list.target.querySelector('.col-name-text')?.textContent).toBe('report')
    expect(list.target.querySelector('.header-date')).toBeTruthy()
    expect(list.target.querySelector('.header-name-ext')).toBeNull()
    expect(headerSortLabels(list.target)).toEqual(['Name', 'Ext', 'Size', 'Modified'])
    // icon 1fr ext size date, mirrored on the header.
    expect(rowTemplate(list.target)).toMatch(/^16px 1fr \d+px \d+px \d+px$/)
    expect(list.target.querySelector<HTMLElement>('.header-row')?.style.gridTemplateColumns).toBe(
      rowTemplate(list.target),
    )
  })

  it('drops the date column (cells and header) first', async () => {
    const list = await mountAt(320)
    await list.settle(() => list.target.querySelectorAll('.col-date').length === 0, 'the date column to drop')
    expect(list.rowNames()).toEqual(['report.md'])
    expect(list.target.querySelectorAll('.col-ext').length).toBe(1)
    expect(list.target.querySelector('.header-date')).toBeNull()
    expect(headerSortLabels(list.target)).toEqual(['Name', 'Ext', 'Size'])
    expect(trackCount(rowTemplate(list.target))).toBe(4)
    expect(list.target.querySelector<HTMLElement>('.header-row')?.style.gridTemplateColumns).toBe(
      rowTemplate(list.target),
    )
  })

  it('then drops Ext, folding the extension into Name and keeping its sort trigger', async () => {
    const list = await mountAt(240)
    await list.settle(() => list.target.querySelectorAll('.col-ext').length === 0, 'the Ext column to drop')
    expect(list.rowNames()).toEqual(['report.md'])
    expect(list.target.querySelectorAll('.col-date').length).toBe(0)
    expect(list.target.querySelector('.col-name-text')?.textContent).toBe('report.md')
    expect(list.target.querySelector('.header-name-ext')).toBeTruthy()
    expect(headerSortLabels(list.target)).toEqual(['Name', 'Ext', 'Size'])
    expect(rowTemplate(list.target)).toMatch(/^16px 1fr \d+px$/)
  })

  // A screen reader hears the same row in any pane width: hiding the column is a sighted-layout
  // tradeoff, and the date costs nothing in the accessible name.
  it('keeps the dropped date in the row’s accessible name', async () => {
    const wide = await mountAt(800)
    const wideName = wide.rows()[0].getAttribute('aria-label')
    expect(wideName).toMatch(/modified/i)
    wide.target.remove()

    const narrow = await mountAt(240)
    await narrow.settle(() => narrow.target.querySelectorAll('.col-date').length === 0, 'the date column to drop')
    expect(narrow.rows()[0].getAttribute('aria-label')).toBe(wideName)
  })

  // A server's share list: its kind's words say the Ext column carries prose and the rows have
  // no dates and no sizes, so the description gets its width and the always-empty date and
  // Size tracks go (a share row's Size could only ever say `<dir>`).
  it('gives a share list’s descriptions their width and drops its empty date and Size columns', async () => {
    const description = 'System default share'
    const list = await mountFullList({
      entries: [
        dirEntry({ name: 'media', path: 'smb://testnas/media', shareMeta: { description }, modifiedAt: undefined }),
      ],
      viewport: { clientHeight: 400, clientWidth: 800, offsetWidth: 800 },
      props: {
        listWords: { extHeader: 'Description', extIsProse: true, rowsHaveNoDates: true, rowsHaveNoSizes: true },
      },
    })
    expect(list.rowNames()).toEqual(['media'])
    expect(list.target.querySelector('.col-ext')?.textContent.trim()).toBe(description)
    expect(list.target.querySelectorAll('.col-date').length).toBe(0)
    expect(list.target.querySelector('.header-date')).toBeNull()
    expect(list.target.querySelectorAll('.col-size').length).toBe(0)
    expect(headerSortLabels(list.target)).toEqual(['Name', 'Description'])
    // icon 1fr ext, the Ext track wide enough for the whole description (7 px per char + 2 px pad).
    expect(trackCount(rowTemplate(list.target))).toBe(3)
    const [, , ext] = rowTemplate(list.target).split(/\s+/)
    expect(ext).toBe(`${String(description.length * 7 + 2)}px`)
    expect(list.target.querySelector<HTMLElement>('.header-row')?.style.gridTemplateColumns).toBe(
      rowTemplate(list.target),
    )
    // Nor does the row's accessible name read out a `<dir>` it no longer shows.
    expect(list.rows()[0].getAttribute('aria-label')).not.toContain('<dir>')
  })

  // A folder's subfolders keep their Size column and its `<dir>` placeholder: only a kind that
  // says its rows carry no sizes loses it.
  it('keeps a folder’s Size column for its subfolders', async () => {
    const list = await mountFullList({
      entries: [dirEntry({ name: 'photos', path: '/test/photos', recursiveSize: undefined })],
      viewport: { clientHeight: 400, clientWidth: 800, offsetWidth: 800 },
    })
    expect(list.target.querySelectorAll('.col-size').length).toBe(1)
    expect(list.target.querySelector('.col-size')?.textContent.trim()).toBe('<dir>')
    expect(headerSortLabels(list.target)).toEqual(['Name', 'Ext', 'Size', 'Modified'])
  })

  it('brings the columns back when the pane widens', async () => {
    const list = await mountAt(240)
    await list.settle(() => list.target.querySelectorAll('.col-date').length === 0, 'the date column to drop')
    list.layout.resize('[data-file-list-surface]', { clientWidth: 800, offsetWidth: 800 })
    await list.settle(() => list.target.querySelectorAll('.col-date').length === 1, 'the date column to return')
    expect(list.target.querySelectorAll('.col-ext').length).toBe(1)
  })
})
