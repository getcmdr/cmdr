/**
 * Where `SettingRow` puts a setting's description: under the label (the default), behind an ⓘ
 * after the label, or a short summary under the label with the ⓘ holding the detail. While
 * Settings search has a query, a tip's text shows under the label too, since search matches it.
 */

import { describe, it, expect, vi, afterEach } from 'vitest'
import { createRawSnippet, mount, tick, type Snippet } from 'svelte'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

const registry = vi.hoisted(() => {
  const def: Record<string, unknown> = { label: 'Theme mode', description: '', descriptionPlacement: 'inline' }
  return { def }
})

const settingsApi = vi.hoisted(() => ({
  getSettingDefinition: vi.fn(() => registry.def),
  resetSetting: vi.fn(),
  isModified: vi.fn(() => false),
  onSpecificSettingChange: vi.fn(() => () => {}),
  onSettingChange: vi.fn(() => () => {}),
}))

vi.mock('#lib/settings/index.js', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ...settingsApi,
}))

import SettingRow from './SettingRow.svelte'

const DETAIL = 'Pick light, dark, or follow the system.'
const SUMMARY = 'How Cmdr looks.'

const control = createRawSnippet(() => ({ render: () => '<input id="theme.mode" />' }))

async function render(
  def: { descriptionPlacement: 'inline' | 'tip'; summary?: string },
  props: { description?: string; searchQuery?: string; split?: boolean; descriptionContent?: Snippet } = {},
) {
  registry.def = { label: 'Theme mode', description: DETAIL, summary: '', ...def }
  const target = document.createElement('div')
  document.body.appendChild(target)
  mount(SettingRow, {
    target,
    props: { id: 'theme.mode', label: 'Theme mode', description: DETAIL, children: control, ...props },
  })
  await tick()
  return target
}

function tipButton(target: HTMLElement) {
  return target.querySelector<HTMLButtonElement>('button.info-tip')
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('SettingRow description placement', () => {
  it('renders the description inline under the label by default, with no tip', async () => {
    const target = await render({ descriptionPlacement: 'inline' }, { split: true })
    const description = target.querySelector('.setting-description')
    expect(description?.textContent).toBe(DETAIL)
    // In the label column, so a split row keeps it beside the control.
    expect(description?.closest('.setting-label-column')).not.toBeNull()
    expect(tipButton(target)).toBeNull()
  })

  it('renders a rich description under the label in place of the plain text', async () => {
    const rich = createRawSnippet(() => ({ render: () => '<span>Rich <a href="#x">link</a></span>' }))
    const target = await render({ descriptionPlacement: 'tip' }, { descriptionContent: rich })
    const lines = target.querySelectorAll('.setting-description')
    expect(lines).toHaveLength(1)
    expect(lines[0].textContent).toBe('Rich link')
    expect(lines[0].closest('.setting-label-column')).not.toBeNull()
    expect(tipButton(target)).toBeNull()
  })

  it('renders no empty paragraph when the setting has no description', async () => {
    const target = await render({ descriptionPlacement: 'inline' }, { description: '' })
    expect(target.querySelector('.setting-description')).toBeNull()
    expect(target.querySelector('p')).toBeNull()
  })

  it('moves a tip setting’s description behind an ⓘ named after the setting', async () => {
    const target = await render({ descriptionPlacement: 'tip' })
    expect(target.querySelector('.setting-description')).toBeNull()
    expect(target.querySelector('p')).toBeNull()
    expect(target.textContent).not.toContain(DETAIL)
    const tip = tipButton(target)
    expect(tip?.getAttribute('aria-label')).toBe('More about “Theme mode”')
    // The ⓘ sits in the label column, right after the label.
    expect(tip?.closest('.setting-label-wrapper')).not.toBeNull()
    await expectNoA11yViolations(target)
  })

  it('shows a tip setting’s summary under the label, in the label column of a split row', async () => {
    const target = await render({ descriptionPlacement: 'tip', summary: SUMMARY }, { split: true })
    const summary = target.querySelector('.setting-description')
    expect(summary?.textContent).toBe(SUMMARY)
    expect(summary?.closest('.setting-label-column')).not.toBeNull()
    expect(target.textContent).not.toContain(DETAIL)
    expect(tipButton(target)).not.toBeNull()
    await expectNoA11yViolations(target)
  })

  it('shows the tip text inline under the label while search has a query', async () => {
    const target = await render({ descriptionPlacement: 'tip', summary: SUMMARY }, { searchQuery: 'dark' })
    const column = target.querySelector('.setting-label-column')
    expect(column?.textContent).toContain(SUMMARY)
    expect(column?.textContent).toContain(DETAIL)
    // The text is on screen, so the ⓘ would only repeat it.
    expect(tipButton(target)).toBeNull()
  })

  it('treats a blank query as no search', async () => {
    const target = await render({ descriptionPlacement: 'tip' }, { searchQuery: '   ' })
    expect(target.textContent).not.toContain(DETAIL)
    expect(tipButton(target)).not.toBeNull()
  })
})
