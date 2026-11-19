import { afterEach, describe, expect, it, vi } from 'vitest'

const calls = vi.hoisted(() => ({ created: 0, webviews: [] as [unknown, string][], menuBars: 0 }))

vi.mock('./backend.js', () => ({
  createDemoBackend: () => {
    calls.created++
    return { id: 'own backend' }
  },
}))
vi.mock('./menu-bar.svelte.js', () => ({
  installMenuBar: () => {
    calls.menuBars++
  },
}))
vi.mock('./webview.js', () => ({
  installWebview: (backend: unknown, label: string) => calls.webviews.push([backend, label]),
}))

const hostDefaults = vi.hoisted(() => ({ set: [] as unknown[] }))
vi.mock('#lib/shortcuts/host-defaults.js', () => ({
  setHostDefaults: (map: unknown) => hostDefaults.set.push(map),
}))

// Imported for its export: under vitest `__CMDR_DEMO_BUILD__` is false, so the module installs nothing by itself.
import { installDemoBackend } from './boot.js'
import { browserHostDefaults } from './browser-shortcuts.js'
import { visitorPlatform } from './platform.js'

type Host = { __cmdrDemoBackend?: unknown }

afterEach(() => {
  calls.created = 0
  calls.webviews = []
  calls.menuBars = 0
  hostDefaults.set = []
  delete (window as unknown as Host).__cmdrDemoBackend
  vi.restoreAllMocks()
})

describe('boot role', () => {
  it('installs nothing on import outside a demo build', () => {
    expect(calls.created).toBe(0)
    expect(calls.webviews).toEqual([])
    expect(hostDefaults.set).toEqual([])
  })

  it("hands every frame's shortcuts store the visitor's host defaults", () => {
    installDemoBackend()
    vi.spyOn(window, 'parent', 'get').mockReturnValue({ __cmdrDemoBackend: { id: 'parent' } } as unknown as Window)
    installDemoBackend()
    const expected = browserHostDefaults(visitorPlatform(navigator.userAgent))
    expect(Object.keys(expected).length).toBeGreaterThan(0)
    expect(hostDefaults.set).toEqual([expected, expected])
  })

  it('makes the top frame the backend and its main webview', () => {
    installDemoBackend()
    expect(calls.created).toBe(1)
    expect(calls.webviews).toEqual([[{ id: 'own backend' }, 'main']])
    expect((window as unknown as Host).__cmdrDemoBackend).toEqual({ id: 'own backend' })
    expect(calls.menuBars).toBe(1)
  })

  it("makes a demo window's iframe a webview over its parent's backend, labeled by its name", () => {
    const shared = { id: 'parent backend' }
    vi.spyOn(window, 'parent', 'get').mockReturnValue({ __cmdrDemoBackend: shared } as unknown as Window)
    vi.spyOn(window, 'name', 'get').mockReturnValue('settings')
    installDemoBackend()
    expect(calls.created).toBe(0)
    expect(calls.webviews).toEqual([[shared, 'settings']])
    // One bar per page, in the top frame.
    expect(calls.menuBars).toBe(0)
  })

  it('is its own top when framed by a page with no backend, or by another origin', () => {
    vi.spyOn(window, 'parent', 'get').mockReturnValue({} as Window)
    installDemoBackend()
    const crossOrigin = new Proxy(
      {},
      {
        get() {
          throw new DOMException('Blocked a frame from accessing a cross-origin frame', 'SecurityError')
        },
      },
    )
    vi.spyOn(window, 'parent', 'get').mockReturnValue(crossOrigin as Window)
    installDemoBackend()
    expect(calls.created).toBe(2)
    expect(calls.webviews.map(([, label]) => label)).toEqual(['main', 'main'])
  })
})
