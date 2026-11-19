import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { initAccentColor, cleanupAccentColor } from './accent-color'
import type { SystemAccent } from '#lib/ipc/bindings.js'

// `vi.mock` is hoisted above the imports, so its factory can only reach these through `vi.hoisted`.
interface MockState {
  accent: SystemAccent
  appColor: 'system' | 'cmdr-gold'
  osListener?: (p: { accent: SystemAccent }) => void
  settingListener?: () => void
}
const state = vi.hoisted((): MockState => ({
  accent: { hex: '#0078d4', fill: null },
  appColor: 'system',
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  getAccentColor: () => Promise.resolve(state.accent),
  onAccentColorChanged: (handler: (p: { accent: SystemAccent }) => void) => {
    state.osListener = handler
    return Promise.resolve(() => {})
  },
}))
vi.mock('#lib/settings/index.js', () => ({
  getSetting: () => state.appColor,
  onSpecificSettingChange: (_key: string, handler: () => void) => {
    state.settingListener = handler
    return () => {}
  },
}))
vi.mock('#lib/icon-cache.js', () => ({ clearDirectoryIconCache: () => Promise.resolve() }))

const WINDOWS_BLUE: SystemAccent = { hex: '#0078d4', fill: { light: '#005a9e', dark: '#99ebff' } }

const root = document.documentElement
const token = (name: string) => root.style.getPropertyValue(name)

let dark = false
let schemeListener: (() => void) | undefined

function switchScheme(toDark: boolean): void {
  dark = toDark
  schemeListener?.()
}

describe('accent color', () => {
  beforeEach(() => {
    state.accent = { hex: '#0078d4', fill: null }
    state.appColor = 'system'
    dark = false
    schemeListener = undefined
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({
        get matches() {
          return dark
        },
        addEventListener: (_: string, listener: () => void) => {
          schemeListener = listener
        },
        removeEventListener: () => {},
      })),
    )
  })

  afterEach(() => {
    cleanupAccentColor()
    root.removeAttribute('style')
    vi.unstubAllGlobals()
  })

  it('uses the accent itself and writes no fill where the OS names no shades', async () => {
    await initAccentColor()
    expect(token('--color-accent')).toBe('#0078d4')
    expect(token('--color-accent-fill')).toBe('')
    expect(token('--color-accent-fill-fg')).toBe('')
  })

  it("fills with the OS's light-theme shade and WinUI's white text in the light scheme", async () => {
    state.accent = WINDOWS_BLUE
    await initAccentColor()
    expect(token('--color-accent')).toBe('#0078d4')
    expect(token('--color-accent-fill')).toBe('#005a9e')
    expect(token('--color-accent-fill-fg')).toBe('#ffffff')
  })

  it('switches to the dark-theme shade and black text when the scheme goes dark, no round-trip', async () => {
    state.accent = WINDOWS_BLUE
    await initAccentColor()
    switchScheme(true)
    expect(token('--color-accent-fill')).toBe('#99ebff')
    expect(token('--color-accent-fill-fg')).toBe('#000000')
  })

  it('follows a live OS accent change, shades included', async () => {
    state.accent = WINDOWS_BLUE
    await initAccentColor()
    state.osListener?.({ accent: { hex: '#e81123', fill: { light: '#c50f1f', dark: '#ff99a4' } } })
    expect(token('--color-accent')).toBe('#e81123')
    expect(token('--color-accent-fill')).toBe('#c50f1f')
  })

  it('drops the fill with the accent when the user picks Cmdr gold', async () => {
    state.accent = WINDOWS_BLUE
    await initAccentColor()
    state.appColor = 'cmdr-gold'
    state.settingListener?.()
    expect(token('--color-accent')).toBe('')
    expect(token('--color-accent-fill')).toBe('')
    expect(token('--color-accent-fill-fg')).toBe('')
  })
})
