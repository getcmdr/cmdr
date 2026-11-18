import { describe, expect, it } from 'vitest'
import {
  CAPABILITIES_GLOBAL,
  CONVENTIONS_GLOBAL,
  CONVENTIONS_ON,
  getPlatform,
  LOOK_GLOBAL,
  NO_CAPABILITIES,
  PLATFORM_GLOBAL,
  platformCapabilities,
  platformConventions,
  type Capability,
} from '#lib/platform/index.js'
import { installPlatform, visitorCapabilities, visitorConventions, visitorPlatform } from './platform.js'

describe('visitorPlatform', () => {
  it.each([
    [
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15',
      'macos',
    ],
    [
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36',
      'windows',
    ],
    ['Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0', 'linux'],
    ['', 'linux'],
  ])('answers %s with %s', (userAgent, platform) => {
    expect(visitorPlatform(userAgent)).toBe(platform)
  })
})

describe('installPlatform', () => {
  it('answers the app for the visitor, read-only as on desktop', () => {
    const frame = { navigator: { userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' } } as unknown as Window
    installPlatform(frame)
    expect((frame as unknown as Record<string, unknown>)[PLATFORM_GLOBAL]).toBe('macos')
    expect(Object.getOwnPropertyDescriptor(frame, PLATFORM_GLOBAL)?.writable).toBe(false)
  })

  it('is what the app reads once installed in its own frame', () => {
    installPlatform(window)
    expect(getPlatform()).toBe(visitorPlatform(navigator.userAgent))
    expect(platformCapabilities()).toEqual(visitorCapabilities(getPlatform()))
  })

  it.each(['Macintosh; Intel Mac OS X 10_15_7', 'Windows NT 10.0; Win64; x64', 'X11; Linux x86_64'])(
    'pins the macOS look (menus and controls) for a %s visitor, under the macOS bar every visitor gets',
    (os) => {
      const frame = { navigator: { userAgent: `Mozilla/5.0 (${os})` } } as unknown as Window
      installPlatform(frame)
      expect((frame as unknown as Record<string, unknown>)[LOOK_GLOBAL]).toBe('macos')
      expect(Object.getOwnPropertyDescriptor(frame, LOOK_GLOBAL)?.writable).toBe(false)
    },
  )

  it('hands the capabilities over read-only too', () => {
    const frame = { navigator: { userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' } } as unknown as Window
    installPlatform(frame)
    expect(Object.getOwnPropertyDescriptor(frame, CAPABILITIES_GLOBAL)?.writable).toBe(false)
    expect(Object.isFrozen((frame as unknown as Record<string, unknown>)[CAPABILITIES_GLOBAL])).toBe(true)
  })

  it('hands the conventions over read-only too, as the app reads them', () => {
    const frame = { navigator: { userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' } } as unknown as Window
    installPlatform(frame)
    expect(Object.getOwnPropertyDescriptor(frame, CONVENTIONS_GLOBAL)?.writable).toBe(false)
    expect(Object.isFrozen((frame as unknown as Record<string, unknown>)[CONVENTIONS_GLOBAL])).toBe(true)
    installPlatform(window)
    expect(platformConventions()).toEqual(visitorConventions(getPlatform()))
  })
})

describe('visitorConventions', () => {
  it('gives each visitor their desktop app’s row, with the macOS bar and right-click menus the demo always draws', () => {
    const mac = { menuModel: 'systemMenuBar', contextMenus: 'native' } as const
    expect(visitorConventions('macos')).toEqual(CONVENTIONS_ON.macos)
    expect(visitorConventions('windows')).toEqual({ ...CONVENTIONS_ON.windows, ...mac })
    expect(visitorConventions('linux')).toEqual({ ...CONVENTIONS_ON.otherLinux, ...mac })
  })

  // Regression anchor (review of #1264): Windows' `inPage` row sent the demo's right-click menus to
  // `open_context_menu_in_page`, which the demo doesn't answer, so a Windows visitor got no menu.
  it("keeps a Windows visitor's right-click menus on the demo's own popups", () => {
    expect(visitorConventions('windows').contextMenus).toBe('native')
  })
})

describe('visitorCapabilities', () => {
  it('shows a Mac visitor the Mac app, overlay title bar and Full Disk Access included', () => {
    const mac = visitorCapabilities('macos')
    for (const capability of Object.keys(NO_CAPABILITIES) as Capability[])
      expect(mac[capability], capability).toBe(true)
  })

  // With `appMenu`, Quit and the Hide trio are the bar's app-menu rows, so no visitor gets a
  // Quit palette row whose IPC has no backend (`command-registry.test.ts` pins that consequence).
  // With `overlayTitleBar`, each page keeps its title line under the traffic lights the demo draws
  // on every window, so the lights never cover a page's own controls (the queue's "Pause all").
  it('shows everyone else no OS-backed surface but the two the demo’s macOS chrome draws', () => {
    const chrome = { ...NO_CAPABILITIES, appMenu: true, overlayTitleBar: true }
    expect(visitorCapabilities('windows')).toEqual(chrome)
    expect(visitorCapabilities('linux')).toEqual(chrome)
  })
})
