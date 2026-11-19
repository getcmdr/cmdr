/**
 * Tests for `createViewerNavigation`: following Markdown links between files in one viewer
 * window, and Back / Forward through them. The IPC boundary and the toast store are mocked;
 * the page's open and swap are spies.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest'

import { createViewerNavigation } from './viewer-navigation.svelte'
import type { MarkdownLink } from '#lib/ipc/bindings.js'

const { followMock, setHistoryStateMock, addToast } = vi.hoisted(() => ({
  followMock: vi.fn<() => Promise<MarkdownLink>>(),
  setHistoryStateMock: vi.fn(() => Promise.resolve()),
  addToast: vi.fn(),
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  viewerFollowMarkdownLink: followMock,
  viewerSetHistoryState: setHistoryStateMock,
  asViewerError: (e: unknown) => (e as { viewerError?: { kind: string } }).viewerError ?? null,
}))
vi.mock('#lib/ui/toast/toast-store.svelte.js', () => ({ addToast }))
vi.mock('#lib/intl/messages.svelte.js', () => ({ tString: (key: string) => key }))

const log = { warn: vi.fn(), debug: vi.fn(), info: vi.fn(), error: vi.fn() }

function setup() {
  let scrollTop = 0
  const open = vi.fn((path: string) => Promise.resolve({ opened: path }))
  const swapTo = vi.fn(() => Promise.resolve())
  const scrollToElement = vi.fn()
  const navigation = createViewerNavigation<{ opened: string }>({
    getSessionId: () => 'sess-1',
    isBusy: () => false,
    getMarkdownScrollTop: () => scrollTop,
    scrollToElement,
    open,
    swapTo,
    log: log as never,
  })
  navigation.start('/docs/README.md')
  return {
    navigation,
    open,
    swapTo,
    scrollToElement,
    scrollTo: (top: number) => {
      scrollTop = top
    },
  }
}

function link(next: MarkdownLink): void {
  followMock.mockResolvedValueOnce(next)
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.spyOn(document, 'hasFocus').mockReturnValue(true)
})

describe('createViewerNavigation', () => {
  it('opens a linked file in the window and lands at its fragment', async () => {
    const { navigation, open, swapTo } = setup()
    link({ kind: 'file', path: '/docs/setup.md', elementId: 'md-install' })

    await navigation.followLink('setup.md#install')

    expect(open).toHaveBeenCalledWith('/docs/setup.md')
    expect(swapTo).toHaveBeenCalledWith({ path: '/docs/setup.md', result: { opened: '/docs/setup.md' } })
    expect(navigation.landing).toEqual({ kind: 'element', elementId: 'md-install' })
    expect(setHistoryStateMock).toHaveBeenLastCalledWith(true, false)
  })

  it('a missing file costs a toast: nothing swaps and the history stays put', async () => {
    const { navigation, open, swapTo } = setup()
    open.mockRejectedValueOnce({ viewerError: { kind: 'notFound' } })
    link({ kind: 'file', path: '/docs/gone.md', elementId: null })

    await navigation.followLink('gone.md')

    expect(swapTo).not.toHaveBeenCalled()
    expect(addToast).toHaveBeenCalledWith('viewer.markdown.link.missing', { level: 'warn' })
    expect(setHistoryStateMock).not.toHaveBeenCalled()
    await navigation.step('back')
    expect(open).toHaveBeenCalledTimes(1)
  })

  it('names a folder for what it is', async () => {
    const { navigation, open } = setup()
    open.mockRejectedValueOnce({ viewerError: { kind: 'isDirectory' } })
    link({ kind: 'file', path: '/docs/guides', elementId: null })

    await navigation.followLink('guides/')

    expect(addToast).toHaveBeenCalledWith('viewer.markdown.link.folder', { level: 'warn' })
  })

  it('goes back to where the document was scrolled, then forward again', async () => {
    const { navigation, open, scrollTo } = setup()
    scrollTo(420)
    link({ kind: 'file', path: '/docs/setup.md', elementId: null })
    await navigation.followLink('setup.md')
    scrollTo(90)

    await navigation.step('back')
    expect(open).toHaveBeenLastCalledWith('/docs/README.md')
    expect(navigation.landing).toEqual({ kind: 'scroll', top: 420 })
    expect(setHistoryStateMock).toHaveBeenLastCalledWith(false, true)

    await navigation.step('forward')
    expect(open).toHaveBeenLastCalledWith('/docs/setup.md')
    expect(navigation.landing).toEqual({ kind: 'scroll', top: 90 })
  })

  it('an anchor scrolls in place and a web link opens nothing here', async () => {
    const { navigation, open, scrollToElement } = setup()
    link({ kind: 'anchor', elementId: 'md-usage' })
    await navigation.followLink('#md-usage')
    link({ kind: 'external', url: 'https://getcmdr.com/' })
    await navigation.followLink('https://getcmdr.com')

    expect(scrollToElement).toHaveBeenCalledWith('md-usage')
    expect(open).not.toHaveBeenCalled()
    expect(addToast).not.toHaveBeenCalled()
  })

  it('says so when a link is one the viewer doesn’t follow', async () => {
    const { navigation } = setup()
    link({ kind: 'blocked' })
    await navigation.followLink('/etc/passwd')
    expect(addToast).toHaveBeenCalledWith('viewer.markdown.link.blocked', { level: 'info' })
  })

  it('leaves the shared menu bar alone while another window has focus', async () => {
    const { navigation } = setup()
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)
    link({ kind: 'file', path: '/docs/setup.md', elementId: null })
    await navigation.followLink('setup.md')
    expect(setHistoryStateMock).not.toHaveBeenCalled()

    navigation.pushHistoryState({ force: true })
    expect(setHistoryStateMock).toHaveBeenCalledWith(true, false)
  })

  it('tells this window’s own page bar every step, focused or not', async () => {
    const { navigation } = setup()
    expect({ back: navigation.canGoBack, forward: navigation.canGoForward }).toEqual({ back: false, forward: false })
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)
    link({ kind: 'file', path: '/docs/setup.md', elementId: null })
    await navigation.followLink('setup.md')
    expect({ back: navigation.canGoBack, forward: navigation.canGoForward }).toEqual({ back: true, forward: false })

    await navigation.step('back')
    expect({ back: navigation.canGoBack, forward: navigation.canGoForward }).toEqual({ back: false, forward: true })
  })
})
