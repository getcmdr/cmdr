import { describe, expect, it } from 'vitest'
import {
  CAPABILITIES_GLOBAL,
  CONVENTIONS_GLOBAL,
  CONVENTIONS_ON,
  getLook,
  getPlatform,
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

  // Task 63 step 11: the look (`data-look`: the menus' and the controls' tokens) is the visitor's own
  // OS's, as their bar is, so nothing pins it.
  it('wears the visitor’s own look', () => {
    installPlatform(window)
    expect(getLook()).toBe(visitorPlatform(navigator.userAgent))
  })

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
  it('gives each visitor their desktop app’s row, menu model included', () => {
    expect(visitorConventions('macos')).toEqual(CONVENTIONS_ON.macos)
    expect(visitorConventions('windows')).toEqual(CONVENTIONS_ON.windows)
    expect(visitorConventions('windows').menuModel).toBe('windowMenuBar')
    expect(visitorConventions('linux')).toEqual(CONVENTIONS_ON.otherLinux)
  })
})

describe('visitorCapabilities', () => {
  it('shows a Mac visitor the Mac app, overlay title bar and Full Disk Access included', () => {
    const mac = visitorCapabilities('macos')
    for (const capability of Object.keys(NO_CAPABILITIES) as Capability[])
      expect(mac[capability], capability).toBe(true)
  })

  // No `appMenu`: their bar is their own OS's, with Quit at the end of File (task 63 step 11). No
  // `overlayTitleBar`: their windows keep the OS's title bar above the page (`title-bar.ts`), so
  // no page leaves room under buttons drawn over it.
  it('shows everyone else no OS-backed surface, as the desktop answers for Windows and Linux', () => {
    expect(visitorCapabilities('windows')).toEqual(NO_CAPABILITIES)
    expect(visitorCapabilities('linux')).toEqual(NO_CAPABILITIES)
  })
})
