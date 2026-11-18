import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { initDisplayAccessibility, cleanupDisplayAccessibility } from './display-accessibility'

interface Options {
  reduceTransparency: boolean
  increaseContrast: boolean
  differentiateWithoutColor: boolean
  invertColors: boolean
  showBorders: boolean
  underlineAccessKeys: boolean
}

const allOff = (): Options => ({
  reduceTransparency: false,
  increaseContrast: false,
  differentiateWithoutColor: false,
  invertColors: false,
  showBorders: false,
  underlineAccessKeys: false,
})

// `vi.mock` is hoisted above the imports, so its factory can only reach these through `vi.hoisted`.
interface MockState {
  options: Options
  listener?: (p: { options: Options }) => void
}
const state = vi.hoisted((): MockState => ({
  options: {
    reduceTransparency: false,
    increaseContrast: false,
    differentiateWithoutColor: false,
    invertColors: false,
    showBorders: false,
    underlineAccessKeys: false,
  },
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  getDisplayAccessibility: () => Promise.resolve(state.options),
  onDisplayAccessibilityChanged: (handler: (p: { options: Options }) => void) => {
    state.listener = handler
    return Promise.resolve(() => {})
  },
}))

const root = document.documentElement
const has = (cls: string) => root.classList.contains(cls)

let mediaMatches = false
let mediaListener: ((e: { matches: boolean }) => void) | undefined

describe('display accessibility', () => {
  beforeEach(() => {
    state.options = allOff()
    mediaMatches = false
    mediaListener = undefined
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({
        get matches() {
          return mediaMatches
        },
        addEventListener: (_: string, fn: (e: { matches: boolean }) => void) => {
          mediaListener = fn
        },
        removeEventListener: () => {
          mediaListener = undefined
        },
      })),
    )
    root.className = ''
  })

  afterEach(() => {
    cleanupDisplayAccessibility()
    vi.unstubAllGlobals()
  })

  it('mirrors each macOS option as a class on <html>', async () => {
    state.options = {
      reduceTransparency: true,
      increaseContrast: true,
      differentiateWithoutColor: true,
      invertColors: true,
      showBorders: true,
      underlineAccessKeys: false,
    }
    await initDisplayAccessibility()
    expect(has('reduce-transparency')).toBe(true)
    expect(has('increase-contrast')).toBe(true)
    expect(has('differentiate-without-color')).toBe(true)
    expect(has('show-borders')).toBe(true)
  })

  it('leaves every class off when macOS reports all off', async () => {
    await initDisplayAccessibility()
    expect(has('reduce-transparency')).toBe(false)
    expect(has('increase-contrast')).toBe(false)
    expect(has('differentiate-without-color')).toBe(false)
    expect(has('show-borders')).toBe(false)
    expect(has('underline-access-keys')).toBe(false)
  })

  it('follows live System Settings changes', async () => {
    await initDisplayAccessibility()
    state.listener?.({ options: { ...allOff(), differentiateWithoutColor: true, showBorders: true } })
    expect(has('differentiate-without-color')).toBe(true)
    expect(has('show-borders')).toBe(true)
    state.listener?.({ options: allOff() })
    expect(has('differentiate-without-color')).toBe(false)
    expect(has('show-borders')).toBe(false)
  })

  it("mirrors Windows' Underline access keys as a class, live", async () => {
    state.options = { ...allOff(), underlineAccessKeys: true }
    await initDisplayAccessibility()
    expect(has('underline-access-keys')).toBe(true)
    state.listener?.({ options: allOff() })
    expect(has('underline-access-keys')).toBe(false)
  })

  it('also turns on increase-contrast from the prefers-contrast media query, live', async () => {
    mediaMatches = true
    await initDisplayAccessibility()
    expect(has('increase-contrast')).toBe(true)
    mediaMatches = false
    mediaListener?.({ matches: false })
    expect(has('increase-contrast')).toBe(false)
  })
})
