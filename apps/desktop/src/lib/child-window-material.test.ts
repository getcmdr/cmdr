import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('#lib/tauri-commands/index.js', () => ({ getShouldReduceTransparency: vi.fn() }))

import { childWindowMaterial } from './child-window-material'
import { withCapabilities, withCapabilitiesPossibleOn } from '#lib/platform/test-platform.js'

let restore: (() => void) | null = null
let dark = false

beforeEach(() => {
  dark = false
  vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('dark') && dark }))
})

afterEach(() => {
  restore?.()
  restore = null
  vi.unstubAllGlobals()
})

const reads = (reduce: boolean) => () => Promise.resolve(reduce)

describe('childWindowMaterial', () => {
  it('opens see-through over the OS material on macOS', async () => {
    restore = withCapabilitiesPossibleOn('macos')
    expect(await childWindowMaterial(reads(false))).toEqual({ glass: true, backgroundColor: [0, 0, 0, 0] })
  })

  it('opens opaque on macOS under Reduce transparency', async () => {
    restore = withCapabilitiesPossibleOn('macos')
    expect(await childWindowMaterial(reads(true))).toEqual({ glass: false, backgroundColor: [255, 255, 255, 255] })
  })

  it('opens opaque where the OS draws no material, Windows and Linux alike', async () => {
    for (const platform of ['windows', 'linux'] as const) {
      restore = withCapabilitiesPossibleOn(platform)
      expect(await childWindowMaterial(reads(false))).toEqual({ glass: false, backgroundColor: [255, 255, 255, 255] })
      restore()
    }
    restore = null
  })

  it('fills an opaque window in the dark appearance', async () => {
    dark = true
    restore = withCapabilities()
    expect((await childWindowMaterial(reads(false))).backgroundColor).toEqual([30, 30, 30, 255])
  })

  it('opens opaque when the Reduce transparency read fails', async () => {
    restore = withCapabilities('windowMaterial')
    const failing = () => Promise.reject(new Error('no backend'))
    expect((await childWindowMaterial(failing)).glass).toBe(false)
  })
})
