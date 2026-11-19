/**
 * Tests for the focus watchdog. Each case sets up the DOM, installs the
 * watchdog, drives focus, and asserts whether a WARN was logged after the
 * timer fires.
 *
 * Uses fake timers so the 500 ms wait doesn't bloat the test runtime.
 *
 * Listeners persist across tests via the shared `document` even after a
 * module re-import, so each test runs `_resetForTests()` in afterEach to
 * detach them before the next install.
 */

import { describe, it, expect, vi, beforeEach, afterEach, type MockInstance } from 'vitest'

// `vi.mock` is hoisted above imports; the factory needs to reach into a
// hoisted scope to share the spy with the test body. `vi.hoisted` is the
// supported escape hatch. Typing the spy's signature here lets each test's
// `warnSpy.mock.calls[0]` destructure as a real tuple instead of `unknown[]`.
const { warnSpy } = vi.hoisted(() => ({
  warnSpy: vi.fn<(message: string, ctx: { ae: string }) => void>(),
}))

vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({
    debug: vi.fn(),
    info: vi.fn(),
    warn: warnSpy,
    error: vi.fn(),
  }),
}))

// Mock the quick-look state so the watchdog can read `isOpen` without dragging
// in the Tauri event API or the IPC command surface. Tests that exercise the
// Quick Look suppression branch flip `quickLookState.isOpen` directly.
const { quickLookState } = vi.hoisted(() => ({ quickLookState: { isOpen: false } }))
vi.mock('#lib/file-explorer/quick-look/quick-look-state.svelte.js', () => ({ quickLookState }))

import { initFocusWatchdog, _resetForTests } from './focus-watchdog'

describe('focus watchdog', () => {
  let hasFocusSpy: MockInstance<() => boolean>

  beforeEach(() => {
    warnSpy.mockClear()
    vi.useFakeTimers()
    document.body.innerHTML = ''
    // jsdom's `document.hasFocus()` returns `false` by default. Force `true`
    // for these tests so the watchdog's "main window not focused" suppression
    // doesn't swallow every case.
    hasFocusSpy = vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    // Reset the Quick Look mock between tests so a flip in one case doesn't
    // leak into the next.
    quickLookState.isOpen = false
  })

  afterEach(() => {
    _resetForTests()
    hasFocusSpy.mockRestore()
    vi.useRealTimers()
    document.body.innerHTML = ''
  })

  it('does not warn when focus is inside the explorer', () => {
    const explorer = document.createElement('div')
    explorer.className = 'dual-pane-explorer'
    explorer.tabIndex = -1
    document.body.appendChild(explorer)
    explorer.focus()

    initFocusWatchdog()
    // Run the initial-check timer and any timer the watchdog might queue.
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  it('does not warn when focus is inside the Ask Cmdr rail', () => {
    // The rail is a keyboard home alongside the panes, so focus in its
    // composer (or any other rail chrome) is a valid resting place.
    const rail = document.createElement('aside')
    rail.className = 'ask-cmdr-rail'
    const composer = document.createElement('textarea')
    composer.className = 'text-field-control'
    rail.appendChild(composer)
    document.body.appendChild(rail)
    composer.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  /** An open in-app menu (a Servers row menu) holds focus on purpose: it warned every time (QA round 2). */
  it('does not warn when focus is inside an open in-app menu', () => {
    const menu = document.createElement('div')
    menu.className = 'menu-surface'
    menu.setAttribute('data-menu', '')
    menu.setAttribute('role', 'menu')
    menu.tabIndex = -1
    document.body.appendChild(menu)
    menu.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  /** A menu's submenu takes focus while it has a cursor, and it's a sibling of `[data-menu]`, not inside it. */
  it('does not warn when focus is inside an open submenu', () => {
    const submenu = document.createElement('div')
    submenu.setAttribute('data-menu-submenu', '')
    submenu.setAttribute('role', 'menu')
    submenu.tabIndex = -1
    document.body.appendChild(submenu)
    submenu.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  /**
   * A command menu (`command-menu/MenuSurface.svelte`: the in-window menu bar's flyout, the main-menu
   * button's menu, an in-page context menu) holds focus on its surface and rows while it's up, so
   * every open warned under the Windows conventions (#1296).
   */
  it('does not warn when focus is on an open command menu or one of its rows', () => {
    const surface = document.createElement('div')
    surface.id = 'command-menu-0'
    surface.className = 'command-menu-surface'
    surface.setAttribute('data-command-menu', 'bar')
    surface.setAttribute('role', 'menu')
    surface.tabIndex = -1
    const row = document.createElement('div')
    row.id = 'command-menu-0-row-3'
    row.className = 'command-menu-row'
    row.setAttribute('role', 'menuitem')
    row.tabIndex = 0
    surface.appendChild(row)
    document.body.appendChild(surface)
    surface.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)
    row.focus()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  /** Alt or F10 hands the in-window menu bar the keyboard: its title holds focus with no menu open. */
  it('does not warn when focus is on a menu-bar title', () => {
    const menuBar = document.createElement('div')
    menuBar.setAttribute('role', 'menubar')
    const title = document.createElement('div')
    title.id = 'menu-bar-c1-title-4'
    title.className = 'menu-bar-title'
    title.setAttribute('role', 'menuitem')
    title.tabIndex = 0
    menuBar.appendChild(title)
    document.body.appendChild(menuBar)
    title.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  /** "Move focus to the notifications" puts focus on a toast, on purpose. */
  it('does not warn when focus is on a toast', () => {
    const stack = document.createElement('div')
    stack.setAttribute('data-toast-stack', '')
    const button = document.createElement('button')
    stack.appendChild(button)
    document.body.appendChild(stack)
    button.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  it('warns when the rail is open but focus falls to body (the bug it must still catch)', () => {
    // Rail being open is NOT a suppressor: if focus lands on <body> while the
    // rail is open, that is still the restore-focus bug the watchdog exists to
    // catch. Only focus actually INSIDE the rail counts as a home.
    const rail = document.createElement('aside')
    rail.className = 'ask-cmdr-rail'
    const composer = document.createElement('textarea')
    composer.className = 'text-field-control'
    rail.appendChild(composer)
    document.body.appendChild(rail)

    document.body.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).toHaveBeenCalledTimes(1)
  })

  it('does not warn when a dialog is open even if focus is loose', () => {
    const dialog = document.createElement('div')
    dialog.setAttribute('role', 'dialog')
    document.body.appendChild(dialog)

    // Focus on body (the symptom we'd otherwise warn about), but the dialog
    // is open, so we should suppress.
    document.body.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  it('warns when focus leaves the explorer, no dialog open, after 500 ms', () => {
    const explorer = document.createElement('div')
    explorer.className = 'dual-pane-explorer'
    explorer.tabIndex = -1
    document.body.appendChild(explorer)
    const stray = document.createElement('input')
    document.body.appendChild(stray)

    // Start with focus in the explorer so the initial check passes.
    explorer.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(150) // past initial-check timer

    // Move focus to an element outside the explorer (simulates a modal that
    // closed and left focus on some stray input).
    stray.focus()

    // Just before the 500 ms threshold, no warn yet.
    vi.advanceTimersByTime(499)
    expect(warnSpy).not.toHaveBeenCalled()

    // Cross the threshold.
    vi.advanceTimersByTime(2)
    expect(warnSpy).toHaveBeenCalledTimes(1)
    const [, ctx] = warnSpy.mock.calls[0]
    expect(ctx.ae).toContain('input')
  })

  it('warns only once per episode and resets when focus returns to a pane', () => {
    const explorer = document.createElement('div')
    explorer.className = 'dual-pane-explorer'
    explorer.tabIndex = -1
    document.body.appendChild(explorer)
    const stray = document.createElement('input')
    document.body.appendChild(stray)
    const otherStray = document.createElement('button')
    document.body.appendChild(otherStray)

    explorer.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(150)

    // Episode 1: lose focus.
    stray.focus()
    vi.advanceTimersByTime(600)
    expect(warnSpy).toHaveBeenCalledTimes(1)

    // Moving focus between stray elements while still loose should NOT
    // trigger a second warn during the same episode.
    otherStray.focus()
    vi.advanceTimersByTime(600)
    expect(warnSpy).toHaveBeenCalledTimes(1)

    // Focus returns to the explorer, resetting the warned flag.
    explorer.focus()
    vi.advanceTimersByTime(100)

    // Lose focus again: should warn anew (new episode).
    stray.focus()
    vi.advanceTimersByTime(600)
    expect(warnSpy).toHaveBeenCalledTimes(2)
  })

  it('does not warn while the Quick Look panel is open', () => {
    // QLPreviewPanel takes key focus, so `document.activeElement` falls back
    // to `<body>` — the same symptom the watchdog warns about. The panel
    // open-state acts as a "dialog" for suppression purposes.
    const stray = document.createElement('input')
    document.body.appendChild(stray)
    document.body.focus()

    quickLookState.isOpen = true
    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  it('does not warn during the initial paint if focus is already inside the explorer', () => {
    const explorer = document.createElement('div')
    explorer.className = 'dual-pane-explorer'
    explorer.tabIndex = -1
    document.body.appendChild(explorer)
    explorer.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  it('does not warn when focus is in the status corner', () => {
    const corner = document.createElement('div')
    corner.className = 'status-corner'
    const chip = document.createElement('button')
    corner.appendChild(chip)
    document.body.appendChild(corner)
    chip.focus()

    initFocusWatchdog()
    vi.advanceTimersByTime(2000)

    expect(warnSpy).not.toHaveBeenCalled()
  })

  describe('says what took focus away', () => {
    function railWith(child: HTMLElement): HTMLElement {
      const rail = document.createElement('aside')
      rail.className = 'ask-cmdr-rail'
      rail.appendChild(child)
      document.body.appendChild(rail)
      return rail
    }

    it('names a focused element that left the page', () => {
      const button = document.createElement('button')
      button.className = 'load-earlier'
      railWith(button)
      initFocusWatchdog()
      button.focus()
      vi.advanceTimersByTime(150)

      // The thread's last page took the button away while it held focus.
      button.remove()
      document.body.dispatchEvent(new FocusEvent('focusin'))
      vi.advanceTimersByTime(600)

      expect(warnSpy).toHaveBeenCalledTimes(1)
      const [message, ctx] = warnSpy.mock.calls[0]
      expect(message).toContain('{cause}')
      expect(ctx).toMatchObject({ ae: 'body', cause: 'button.load-earlier left the page while it had focus' })
    })

    it('names a focused element that got disabled', () => {
      const button = document.createElement('button')
      button.className = 'send'
      railWith(button)
      initFocusWatchdog()
      button.focus()
      vi.advanceTimersByTime(150)

      // WebKit drops focus from a button that gets disabled; jsdom keeps it there, so the test
      // blurs it by hand first (the cause is read at warn time, so the order doesn't matter).
      button.blur()
      button.disabled = true
      vi.advanceTimersByTime(600)

      expect(warnSpy.mock.calls[0][1]).toMatchObject({ cause: 'button.send was disabled while it had focus' })
    })

    it('names a click that put focus nowhere (WebKit doesn’t focus a clicked button)', () => {
      const input = document.createElement('textarea')
      railWith(input)
      const stray = document.createElement('button')
      stray.className = 'fkey'
      document.body.appendChild(stray)
      initFocusWatchdog()
      input.focus()
      vi.advanceTimersByTime(150)

      // jsdom has no PointerEvent constructor; the watchdog reads only the target.
      stray.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
      input.blur()
      vi.advanceTimersByTime(600)

      expect(warnSpy.mock.calls[0][1]).toMatchObject({
        cause: 'a click on button.fkey took it from textarea',
      })
    })

    it('falls back to naming what last had focus', () => {
      const input = document.createElement('textarea')
      railWith(input)
      initFocusWatchdog()
      input.focus()
      vi.advanceTimersByTime(150)

      input.blur()
      vi.advanceTimersByTime(600)

      expect(warnSpy.mock.calls[0][1]).toMatchObject({ cause: 'textarea lost it' })
    })
  })
})
