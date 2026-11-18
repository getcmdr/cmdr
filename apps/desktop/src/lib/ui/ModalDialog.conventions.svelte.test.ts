/**
 * `ModalDialog` follows the platform's dialog conventions (`#lib/platform`'s `convention()`) on
 * every desktop row: button order, Tab order, Enter and Escape, and access keys. The macOS block
 * pins today's markup and behavior, so a convention can only ever change the other rows.
 */

import { afterEach, describe, expect, it, vi } from 'vitest'
import { createRawSnippet, mount, tick, unmount } from 'svelte'
import ModalDialog from './ModalDialog.svelte'
import { _resetForTests as resetFocusTraps } from './focus-trap'
import { withConventions } from '#lib/platform/test-platform.js'
import type { ConventionsDesktop } from '#lib/platform/index.js'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

vi.mock('#lib/tauri-commands/index.js', () => ({
  notifyDialogOpened: vi.fn(() => Promise.resolve()),
  notifyDialogClosed: vi.fn(() => Promise.resolve()),
}))

const title = createRawSnippet(() => ({ render: () => `<span>Save changes</span>` }))
const body = createRawSnippet(() => ({
  render: () =>
    `<div><label for="name-field">Name</label><input id="name-field" /><button id="options">Options</button></div>`,
}))
/**
 * Two footer buttons as two direct children of the footer row, written in macOS order (Cancel,
 * then Save), the shape every dialog's footer has. A raw snippet renders one root element, so
 * the second button joins it as its sibling.
 */
const footer = createRawSnippet(() => ({
  render: () => `<button id="cancel">Cancel</button>`,
  setup: (node: Element) => {
    const save = document.createElement('button')
    save.id = 'save'
    save.textContent = 'Save'
    node.after(save)
  },
}))

const ALL_DESKTOPS: ConventionsDesktop[] = ['macos', 'windows', 'gnome', 'kde', 'otherLinux']

interface Opened {
  overlay: HTMLElement
  onkeydown: ReturnType<typeof vi.fn>
  onclose: ReturnType<typeof vi.fn>
}

let teardowns: (() => void)[] = []

afterEach(() => {
  for (const teardown of teardowns) teardown()
  teardowns = []
  resetFocusTraps()
  document.body.innerHTML = ''
})

async function open(extra: { letterKeysAnswer?: boolean } = {}): Promise<Opened> {
  const target = document.createElement('div')
  document.body.appendChild(target)
  const onkeydown = vi.fn()
  const onclose = vi.fn()
  const component = mount(ModalDialog, {
    target,
    props: { titleId: 'conv-title', title, children: body, footer, onkeydown, onclose, ...extra },
  })
  await tick()
  await tick()
  const overlay = target.querySelector<HTMLElement>('.modal-overlay')
  if (!overlay) throw new Error('no overlay rendered')
  teardowns.push(() => {
    void unmount(component)
    target.remove()
  })
  return { overlay, onkeydown, onclose }
}

/**
 * The markup a person's browser lays out: without Svelte's per-stylesheet hash classes, which move
 * whenever the CSS does, and without its block anchors (empty comments), which render nothing.
 */
function markupOf(el: Element): string {
  return el.outerHTML
    .replace(/ ?svelte-[a-z0-9]+/g, '')
    .replace(/ class=""/g, '')
    .replace(/<!---->/g, '')
}

function press(el: Element, init: KeyboardEventInit): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })
  el.dispatchEvent(event)
  return event
}

function byId(root: ParentNode, id: string): HTMLElement {
  const el = root.querySelector<HTMLElement>(`#${id}`)
  if (!el) throw new Error(`#${id} not rendered`)
  return el
}

/**
 * Presses Tab `count` times from the focused element and lists where focus went. jsdom moves no
 * focus itself, so a Tab nothing claimed moves to the next control in DOM order, as a browser does.
 */
function tabWalk(overlay: HTMLElement, count: number, shiftKey = false): string[] {
  const visited = [document.activeElement?.id ?? '']
  for (let i = 0; i < count; i++) {
    const active = document.activeElement as HTMLElement
    const event = press(active, { key: 'Tab', shiftKey })
    if (!event.defaultPrevented) {
      const all = [...overlay.querySelectorAll<HTMLElement>('input, button:not([tabindex="-1"])')]
      all[all.indexOf(active) + (shiftKey ? -1 : 1)]?.focus()
    }
    visited.push(document.activeElement?.id ?? '')
  }
  return visited
}

describe('ModalDialog on macOS (characterization: what every Mac user has today)', () => {
  it('renders this exact markup', async () => {
    withConventions('macos')
    const { overlay } = await open()
    expect(markupOf(overlay)).toMatchSnapshot()
  })

  it('Tabs through the controls in DOM order and wraps', async () => {
    withConventions('macos')
    const { overlay } = await open()
    byId(overlay, 'name-field').focus()
    expect(tabWalk(overlay, 4)).toEqual(['name-field', 'options', 'cancel', 'save', 'name-field'])
  })

  it('Escape closes, Enter on a focused button presses that button, Enter elsewhere reaches the dialog', async () => {
    withConventions('macos')
    const { overlay, onclose, onkeydown } = await open()
    press(byId(overlay, 'name-field'), { key: 'Escape' })
    expect(onclose).toHaveBeenCalledTimes(1)
    press(byId(overlay, 'cancel'), { key: 'Enter' })
    expect(onkeydown).not.toHaveBeenCalled()
    press(byId(overlay, 'name-field'), { key: 'Enter' })
    expect(onkeydown).toHaveBeenCalledTimes(1)
  })

  it('hands Option keys to the dialog untouched', async () => {
    withConventions('macos')
    const { overlay, onkeydown } = await open()
    const clicked = vi.fn()
    byId(overlay, 'save').addEventListener('click', clicked)
    press(byId(overlay, 'name-field'), { key: 'Alt', code: 'AltLeft', altKey: true })
    const letter = press(byId(overlay, 'name-field'), { key: 's', code: 'KeyS', altKey: true })
    expect(clicked).not.toHaveBeenCalled()
    expect(letter.defaultPrevented).toBe(false)
    expect(onkeydown).toHaveBeenCalledTimes(2)
    expect(markupOf(overlay)).not.toContain('aria-keyshortcuts')
  })
})

/** The footer row as Tab and the eye read it. */
function footerRow(overlay: HTMLElement): HTMLElement {
  const row = overlay.querySelector<HTMLElement>('.modal-footer-actions')
  if (!row) throw new Error('no footer row')
  return row
}

const BUTTON_ORDER: Record<ConventionsDesktop, 'cancelFirst' | 'affirmativeFirst'> = {
  macos: 'cancelFirst',
  windows: 'affirmativeFirst',
  gnome: 'cancelFirst',
  kde: 'affirmativeFirst',
  otherLinux: 'cancelFirst',
}

describe.each(ALL_DESKTOPS)('ModalDialog on %s', (desktop) => {
  const reversed = BUTTON_ORDER[desktop] === 'affirmativeFirst'

  it(reversed ? 'shows the affirmative button first' : 'shows Cancel first', async () => {
    withConventions(desktop)
    const { overlay } = await open()
    const row = footerRow(overlay)
    expect(row.classList.contains('affirmative-first')).toBe(reversed)
    expect(row.getAttribute('data-focus-order')).toBe(reversed ? 'reverse' : null)
  })

  it('Tabs in the order the buttons show, both ways, wrapping', async () => {
    withConventions(desktop)
    const { overlay } = await open()
    byId(overlay, 'name-field').focus()
    const footerOrder = reversed ? ['save', 'cancel'] : ['cancel', 'save']
    expect(tabWalk(overlay, 4)).toEqual(['name-field', 'options', ...footerOrder, 'name-field'])
    expect(tabWalk(overlay, 4, true)).toEqual(['name-field', ...[...footerOrder].reverse(), 'options', 'name-field'])
  })

  it('has no a11y violations', async () => {
    withConventions(desktop)
    const { overlay } = await open()
    await expectNoA11yViolations(overlay)
  })
})

describe.each(['windows', 'gnome', 'kde', 'otherLinux'] as const)('ModalDialog access keys on %s', (desktop) => {
  it('names every labeled control its Alt key, for screen readers', async () => {
    withConventions(desktop)
    const { overlay } = await open()
    await new Promise((resolve) => requestAnimationFrame(resolve))
    // DOM order, word initials first: Name, Options, Cancel, then Save's S.
    expect(byId(overlay, 'name-field').getAttribute('aria-keyshortcuts')).toBe('Alt+N')
    expect(byId(overlay, 'options').getAttribute('aria-keyshortcuts')).toBe('Alt+O')
    expect(byId(overlay, 'cancel').getAttribute('aria-keyshortcuts')).toBe('Alt+C')
    expect(byId(overlay, 'save').getAttribute('aria-keyshortcuts')).toBe('Alt+S')
  })

  it('shows keytips while Alt is held and hides them on release', async () => {
    withConventions(desktop)
    const { overlay } = await open()
    press(byId(overlay, 'name-field'), { key: 'Alt', code: 'AltLeft', altKey: true })
    await tick()
    expect([...overlay.querySelectorAll('.access-key-tip')].map((tip) => tip.textContent)).toEqual(['N', 'O', 'C', 'S'])
    window.dispatchEvent(new KeyboardEvent('keyup', { key: 'Alt', code: 'AltLeft' }))
    await tick()
    expect(overlay.querySelector('.access-key-tip')).toBeNull()
  })

  it('presses a button and focuses a field from its access key', async () => {
    withConventions(desktop)
    const { overlay } = await open()
    const clicked = vi.fn()
    byId(overlay, 'save').addEventListener('click', clicked)
    const save = press(byId(overlay, 'name-field'), { key: 's', code: 'KeyS', altKey: true })
    expect(clicked).toHaveBeenCalledTimes(1)
    expect(save.defaultPrevented).toBe(true)
    press(byId(overlay, 'save'), { key: 'n', code: 'KeyN', altKey: true })
    expect(document.activeElement?.id).toBe('name-field')
  })

  it('leaves an Alt key the dialog claimed to the dialog', async () => {
    withConventions(desktop)
    const { overlay, onkeydown } = await open()
    onkeydown.mockImplementation((event: KeyboardEvent) => {
      if (event.key === 's') event.preventDefault()
    })
    const clicked = vi.fn()
    byId(overlay, 'save').addEventListener('click', clicked)
    press(byId(overlay, 'name-field'), { key: 's', code: 'KeyS', altKey: true })
    expect(clicked).not.toHaveBeenCalled()
  })

  it('gives a dialog whose bare letters answer it no second set of keys', async () => {
    withConventions(desktop)
    const { overlay } = await open({ letterKeysAnswer: true })
    await new Promise((resolve) => requestAnimationFrame(resolve))
    const clicked = vi.fn()
    byId(overlay, 'save').addEventListener('click', clicked)
    press(byId(overlay, 'name-field'), { key: 's', code: 'KeyS', altKey: true })
    expect(clicked).not.toHaveBeenCalled()
    expect(markupOf(overlay)).not.toContain('aria-keyshortcuts')
  })

  it('ignores AltGr, which types characters', async () => {
    withConventions(desktop)
    const { overlay } = await open()
    const clicked = vi.fn()
    byId(overlay, 'save').addEventListener('click', clicked)
    press(byId(overlay, 'name-field'), { key: 's', code: 'KeyS', altKey: true, ctrlKey: true })
    expect(clicked).not.toHaveBeenCalled()
  })
})
