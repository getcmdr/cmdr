/**
 * `ModalDialog` follows the platform's dialog conventions (`#lib/platform`'s `convention()`) on
 * every desktop row: button order, Tab order, Return and Escape (and ⌘. on the Mac), the default
 * button, and access keys (keytips on Windows, underlines on Linux). The macOS block pins the Mac
 * markup and keys, so a convention change can't move them unnoticed.
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
const footer = footerWith()

/**
 * The same footer, with Save marked as the dialog's default button the way `Button`'s `isDefault`
 * marks it (`data-dialog-default`, and `Enter` announced). `'destructive'` stands for a danger
 * default, which Return treats the same way.
 */
function footerWith(defaultSave?: 'default' | 'destructive') {
  return createRawSnippet(() => ({
    render: () => `<button id="cancel">Cancel</button>`,
    setup: (node: Element) => {
      const save = document.createElement('button')
      save.id = 'save'
      save.textContent = 'Save'
      if (defaultSave) {
        save.setAttribute('data-dialog-default', '')
        save.setAttribute('aria-keyshortcuts', 'Enter')
      }
      node.after(save)
    },
  }))
}

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

async function open(
  extra: {
    letterKeysAnswer?: boolean
    footer?: ReturnType<typeof footerWith>
    children?: ReturnType<typeof createRawSnippet>
  } = {},
): Promise<Opened> {
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

  it('Escape and ⌘. close; Return never presses the focused button, it reaches the dialog from a field', async () => {
    withConventions('macos')
    const { overlay, onclose, onkeydown } = await open()
    press(byId(overlay, 'name-field'), { key: 'Escape' })
    expect(onclose).toHaveBeenCalledTimes(1)
    const period = press(byId(overlay, 'name-field'), { key: '.', code: 'Period', metaKey: true })
    expect(onclose).toHaveBeenCalledTimes(2)
    expect(period.defaultPrevented).toBe(true)
    // Return on a footer button that isn't the default does nothing, not even reach the dialog.
    const onCancel = press(byId(overlay, 'cancel'), { key: 'Enter' })
    expect(onkeydown).not.toHaveBeenCalled()
    expect(onCancel.defaultPrevented).toBe(true)
    press(byId(overlay, 'name-field'), { key: 'Enter' })
    expect(onkeydown).toHaveBeenCalledTimes(1)
  })

  it('lets a body button (a list row, a chip) take Return as its own press', async () => {
    withConventions('macos')
    const { overlay, onkeydown } = await open({ footer: footerWith('default') })
    const saved = vi.fn()
    byId(overlay, 'save').addEventListener('click', saved)
    const enter = press(byId(overlay, 'options'), { key: 'Enter' })
    expect(enter.defaultPrevented).toBe(false)
    expect(onkeydown).not.toHaveBeenCalled()
    expect(saved).not.toHaveBeenCalled()
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

const ENTER_PRESSES_FOCUSED: Record<ConventionsDesktop, boolean> = {
  macos: false,
  windows: true,
  gnome: true,
  kde: true,
  otherLinux: true,
}

const textareaBody = createRawSnippet(() => ({
  render: () => `<div><label for="note">Note</label><textarea id="note"></textarea></div>`,
}))

describe.each(ALL_DESKTOPS)('ModalDialog Return and the default button on %s', (desktop) => {
  const pressesFocused = ENTER_PRESSES_FOCUSED[desktop]

  it('presses the default button from a field, once', async () => {
    withConventions(desktop)
    const { overlay, onkeydown } = await open({ footer: footerWith('default') })
    const saved = vi.fn()
    byId(overlay, 'save').addEventListener('click', saved)
    const enter = press(byId(overlay, 'name-field'), { key: 'Enter' })
    expect(onkeydown).toHaveBeenCalledTimes(1)
    expect(saved).toHaveBeenCalledTimes(1)
    expect(enter.defaultPrevented).toBe(true)
  })

  it('leaves Return the dialog claimed to the dialog', async () => {
    withConventions(desktop)
    const { overlay, onkeydown } = await open({ footer: footerWith('default') })
    onkeydown.mockImplementation((event: KeyboardEvent) => {
      if (event.key === 'Enter') event.preventDefault()
    })
    const saved = vi.fn()
    byId(overlay, 'save').addEventListener('click', saved)
    press(byId(overlay, 'name-field'), { key: 'Enter' })
    expect(saved).not.toHaveBeenCalled()
  })

  it(
    pressesFocused
      ? 'lets a focused button take Return as its own press'
      : 'does nothing on Return from a footer button that isn’t the default',
    async () => {
      withConventions(desktop)
      const { overlay, onkeydown } = await open({ footer: footerWith('default') })
      const saved = vi.fn()
      byId(overlay, 'save').addEventListener('click', saved)
      const enter = press(byId(overlay, 'cancel'), { key: 'Enter' })
      // A browser presses the focused button unless the keydown was prevented. On the Mac
      // (Apple's HIG) Return presses only the default button: never the focused Cancel, and never
      // the default from Cancel either (someone who moved to Cancel meant to cancel).
      expect(enter.defaultPrevented).toBe(!pressesFocused)
      expect(saved).not.toHaveBeenCalled()
      expect(onkeydown).not.toHaveBeenCalled()
    },
  )

  it('presses the default button on Return when it has focus itself, once', async () => {
    withConventions(desktop)
    const { overlay } = await open({ footer: footerWith('default') })
    const saved = vi.fn()
    byId(overlay, 'save').addEventListener('click', saved)
    const enter = press(byId(overlay, 'save'), { key: 'Enter' })
    // Windows and Linux: the browser presses the focused default. The Mac: the dialog does.
    expect(saved).toHaveBeenCalledTimes(pressesFocused ? 0 : 1)
    expect(enter.defaultPrevented).toBe(!pressesFocused)
  })

  it('never moves a destructive default onto Return pressed on another button', async () => {
    withConventions(desktop)
    const { overlay, onkeydown } = await open({ footer: footerWith('destructive') })
    const saved = vi.fn()
    byId(overlay, 'save').addEventListener('click', saved)
    const enter = press(byId(overlay, 'cancel'), { key: 'Enter' })
    expect(saved).not.toHaveBeenCalled()
    expect(onkeydown).not.toHaveBeenCalled()
    // Windows and Linux: Cancel takes its own press. The Mac: nothing happens at all.
    expect(enter.defaultPrevented).toBe(!pressesFocused)
  })

  it('keeps Return in a multi-line field a new line', async () => {
    withConventions(desktop)
    const { overlay } = await open({ footer: footerWith('default'), children: textareaBody })
    const saved = vi.fn()
    byId(overlay, 'save').addEventListener('click', saved)
    const enter = press(byId(overlay, 'note'), { key: 'Enter' })
    expect(saved).not.toHaveBeenCalled()
    expect(enter.defaultPrevented).toBe(false)
  })

  it('without a default button, Return presses nothing but what the platform gives the focus', async () => {
    withConventions(desktop)
    const { overlay } = await open()
    const enter = press(byId(overlay, 'cancel'), { key: 'Enter' })
    expect(enter.defaultPrevented).toBe(!pressesFocused)
  })

  it(desktop === 'macos' ? 'cancels on ⌘.' : 'leaves ⌘. alone', async () => {
    withConventions(desktop)
    const { overlay, onclose } = await open()
    press(byId(overlay, 'name-field'), { key: '.', code: 'Period', metaKey: true })
    expect(onclose).toHaveBeenCalledTimes(desktop === 'macos' ? 1 : 0)
  })

  it('announces the default button as pressed by Return, and passes axe', async () => {
    withConventions(desktop)
    const { overlay } = await open({ footer: footerWith('default') })
    await new Promise((resolve) => requestAnimationFrame(resolve))
    const keys = byId(overlay, 'save').getAttribute('aria-keyshortcuts') ?? ''
    // Where there are access keys, the default button's own sits beside Enter.
    expect(keys).toBe(desktop === 'macos' ? 'Enter' : 'Enter Alt+S')
    await expectNoA11yViolations(overlay)
  })
})

/** A stand-in for the CSS Custom Highlight API, which happy-dom lacks: records what's painted. */
function stubHighlights(): Map<string, { ranges: Range[] }> {
  const registry = new Map<string, { ranges: Range[] }>()
  vi.stubGlobal('CSS', { highlights: { set: registry.set.bind(registry), delete: registry.delete.bind(registry) } })
  vi.stubGlobal(
    'Highlight',
    class {
      ranges: Range[]
      constructor(...ranges: Range[]) {
        this.ranges = ranges
      }
    },
  )
  return registry
}

/** A footer whose labels have no letter a keyboard types (Chinese), so each takes one from a to z. */
const cjkFooter = createRawSnippet(() => ({
  render: () => `<button id="cancel">取消</button>`,
  setup: (node: Element) => {
    const save = document.createElement('button')
    save.id = 'save'
    save.textContent = '保存'
    node.after(save)
  },
}))

describe.each(['gnome', 'kde', 'otherLinux'] as const)(
  'ModalDialog access keys on %s look like GTK and Qt',
  (desktop) => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('underlines each letter in its label while Alt is held, and draws no keytips', async () => {
      const registry = stubHighlights()
      withConventions(desktop)
      const { overlay } = await open()
      press(byId(overlay, 'name-field'), { key: 'Alt', code: 'AltLeft', altKey: true })
      await tick()
      const underlined = registry.get('cmdr-access-key')?.ranges.map((range) => range.toString())
      expect(underlined).toEqual(['N', 'O', 'C', 'S'])
      expect(overlay.querySelector('.access-key-tip')).toBeNull()
      window.dispatchEvent(new KeyboardEvent('keyup', { key: 'Alt', code: 'AltLeft' }))
      expect(registry.has('cmdr-access-key')).toBe(false)
    })

    it('underlines the occurrence the letter came from: a word initial before an earlier letter', async () => {
      const registry = stubHighlights()
      withConventions(desktop)
      const words = createRawSnippet(() => ({
        render: () => `<div><button id="first">Copy</button><button id="second">Copy over</button></div>`,
      }))
      const { overlay } = await open({ children: words })
      press(overlay, { key: 'Alt', code: 'AltLeft', altKey: true })
      const ranges = registry.get('cmdr-access-key')?.ranges ?? []
      // "Copy" takes C, so "Copy over" takes over's O, the word initial, ❌ not the o in "Copy".
      const second = ranges.find((range) => range.startContainer.parentElement?.id === 'second')
      expect(second?.toString()).toBe('o')
      expect(second?.startOffset).toBe(5)
    })

    it('writes a label without its letter as "保存 (B)", hidden from the accessible name', async () => {
      const registry = stubHighlights()
      withConventions(desktop)
      const { overlay } = await open({ footer: cjkFooter })
      await new Promise((resolve) => requestAnimationFrame(resolve))
      const suffix = byId(overlay, 'save').querySelector('.access-key-suffix')
      // Name, Options, then 取消 takes a, 保存 takes b.
      expect(suffix?.textContent).toBe(' (B)')
      expect(suffix?.getAttribute('aria-hidden')).toBe('true')
      expect(byId(overlay, 'save').getAttribute('aria-keyshortcuts')).toBe('Alt+B')
      press(byId(overlay, 'name-field'), { key: 'Alt', code: 'AltLeft', altKey: true })
      const underlined = registry
        .get('cmdr-access-key')
        ?.ranges.find((range) => range.startContainer.parentElement === suffix)
      expect(underlined?.toString()).toBe('B')
      await expectNoA11yViolations(overlay)
    })

    it('falls back to keytips where the Highlight API is missing', async () => {
      withConventions(desktop)
      const { overlay } = await open()
      press(byId(overlay, 'name-field'), { key: 'Alt', code: 'AltLeft', altKey: true })
      await tick()
      expect(overlay.querySelectorAll('.access-key-tip')).toHaveLength(4)
    })
  },
)

describe('ModalDialog access keys on windows look like Windows 11', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('draws keytips even where underlines could paint, and appends nothing to a label', async () => {
    const registry = stubHighlights()
    withConventions('windows')
    const { overlay } = await open({ footer: cjkFooter })
    await new Promise((resolve) => requestAnimationFrame(resolve))
    expect(overlay.querySelector('.access-key-suffix')).toBeNull()
    press(byId(overlay, 'name-field'), { key: 'Alt', code: 'AltLeft', altKey: true })
    await tick()
    expect(overlay.querySelectorAll('.access-key-tip')).toHaveLength(4)
    expect(registry.has('cmdr-access-key')).toBe(false)
  })
})
