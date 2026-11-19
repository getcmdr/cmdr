import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  createMainWindowTitleBar,
  createTitleBar,
  MAIN_WINDOW_TITLE,
  TITLE_BAR_HEIGHT,
  TITLE_BAR_HEIGHT_PROPERTY,
} from './title-bar.js'

const button = (root: HTMLElement, kind: string) => root.querySelector<HTMLButtonElement>(`[data-button="${kind}"]`)

afterEach(() => {
  document.documentElement.style.removeProperty(TITLE_BAR_HEIGHT_PROPERTY)
  Reflect.deleteProperty(document, 'fullscreenEnabled')
  Reflect.deleteProperty(document.documentElement, 'requestFullscreen')
})

describe('the Windows and Linux title bars', () => {
  it('draws the title and the OS’s three buttons, in the visitor’s look, no Tab stops', () => {
    for (const look of ['windows', 'linux'] as const) {
      const { root } = createTitleBar(look, 'Settings', {})
      expect(root.dataset.look).toBe(look)
      expect(root.textContent).toBe('Settings')
      const kinds = [...root.querySelectorAll<HTMLButtonElement>('button')].map((b) => b.dataset.button)
      expect(kinds).toEqual(['minimize', 'maximize', 'close'])
      for (const b of root.querySelectorAll('button')) expect(b.tabIndex).toBe(-1)
    }
    expect(document.getElementById('cmdr-demo-titlebar-style')).not.toBeNull()
  })

  it('runs a button’s action, and leaves one with no action inert', () => {
    const close = vi.fn()
    const { root } = createTitleBar('windows', 'Settings', { close })
    button(root, 'close')?.click()
    expect(close).toHaveBeenCalledOnce()
    expect(button(root, 'minimize')?.getAttribute('aria-disabled')).toBe('true')
    expect(button(root, 'close')?.hasAttribute('aria-disabled')).toBe(false)
  })

  it('moves the window from a press on the bar, and maximizes on a double click, never from a button', () => {
    const drag = vi.fn()
    const maximize = vi.fn()
    const { root } = createTitleBar('linux', 'Settings', { drag, maximize })
    root.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }))
    expect(drag).toHaveBeenCalledOnce()
    button(root, 'close')?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }))
    expect(drag).toHaveBeenCalledOnce()
    root.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
    expect(maximize).toHaveBeenCalledOnce()
  })

  it('dims for an inactive window and swaps maximize for restore while zoomed', () => {
    const bar = createTitleBar('windows', 'Settings', { maximize: () => {} })
    bar.setActive(false)
    expect(bar.root.dataset.active).toBe('false')
    bar.setZoomGlyph('exitFullScreen')
    expect(bar.root.dataset.zoom).toBe('exitFullScreen')
    expect(button(bar.root, 'maximize')?.getAttribute('aria-label')).toBe('Restore')
    bar.setZoomGlyph('enterFullScreen')
    expect(button(bar.root, 'maximize')?.getAttribute('aria-label')).toBe('Maximize')
  })

  it('titles the main window as tauri.conf.json does, and sets its height for the chrome under it', () => {
    const bar = createMainWindowTitleBar('windows')
    expect(bar.root.textContent).toBe(MAIN_WINDOW_TITLE)
    expect(MAIN_WINDOW_TITLE).toBe('Cmdr')
    expect(document.documentElement.style.getPropertyValue(TITLE_BAR_HEIGHT_PROPERTY)).toBe(
      `${String(TITLE_BAR_HEIGHT.windows)}px`,
    )
    // A page can't hide or close its own tab; with no full screen to offer, maximize is inert too.
    for (const kind of ['minimize', 'maximize', 'close'])
      expect(button(bar.root, kind)?.getAttribute('aria-disabled'), kind).toBe('true')
  })

  it('maximizes the main window into the browser’s full screen where the page may', () => {
    const request = vi.fn(() => Promise.resolve())
    Object.defineProperty(document, 'fullscreenEnabled', { configurable: true, value: true })
    Object.defineProperty(document.documentElement, 'requestFullscreen', { configurable: true, value: request })
    const bar = createMainWindowTitleBar('linux')
    button(bar.root, 'maximize')?.click()
    expect(request).toHaveBeenCalledOnce()
  })
})
