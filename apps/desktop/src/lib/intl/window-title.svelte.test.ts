/**
 * A secondary window's OS title follows a live language change. The title is
 * a native `setTitle` write, so nothing re-renders it the way markup re-renders:
 * `followWindowTitle` is the one place that re-writes it when the locale moves.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushSync } from 'svelte'

vi.mock('#lib/app-mode.js', () => ({
  initAppMode: () => Promise.resolve('prod'),
  decorateChildWindowTitle: (title: string) => `[${title}]`,
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setTitle: () => Promise.resolve() }),
}))

import { followWindowTitle } from './window-title.svelte'
import { setLocale, tString, _setCatalogForTests, _clearCompiledCacheForTests } from './messages.svelte'
import { _setLocaleForTests } from './locale'

/** A test-only catalog, so a switch actually CHANGES the title. */
const TEST_LANG = 'zz'

/** Lets `initAppMode()` resolve, so the follower is live. */
const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

beforeEach(() => {
  void setLocale('en')
  _setCatalogForTests(TEST_LANG, { 'settings.window.title': 'Beállítások' })
})

afterEach(() => {
  void setLocale(null)
  _setLocaleForTests(null)
  _setCatalogForTests(TEST_LANG, null)
  _clearCompiledCacheForTests()
})

describe('followWindowTitle', () => {
  it('writes the decorated title once the app mode is known', async () => {
    const write = vi.fn(() => Promise.resolve())
    const stop = followWindowTitle(() => tString('settings.window.title'), write)
    await settle()
    flushSync()

    expect(write).toHaveBeenCalledWith('[Settings]')
    stop()
  })

  it('re-writes the title when the language changes', async () => {
    const write = vi.fn(() => Promise.resolve())
    const stop = followWindowTitle(() => tString('settings.window.title'), write)
    await settle()
    flushSync()

    void setLocale(TEST_LANG)
    flushSync()

    expect(write).toHaveBeenLastCalledWith('[Beállítások]')
    stop()
  })

  it('skips the write when a locale bump leaves the title as it was', async () => {
    // A region change bumps the version rune with the copy unchanged; the OS
    // title shouldn't take a round trip for nothing.
    const write = vi.fn(() => Promise.resolve())
    const stop = followWindowTitle(() => tString('settings.window.title'), write)
    await settle()
    flushSync()

    void setLocale('en')
    flushSync()

    expect(write).toHaveBeenCalledTimes(1)
    stop()
  })

  it('leaves the title alone while the getter has nothing to say', async () => {
    // The viewer, before its file loads: the opener's bare filename stays.
    const write = vi.fn(() => Promise.resolve())
    const stop = followWindowTitle(() => null, write)
    await settle()
    flushSync()

    expect(write).not.toHaveBeenCalled()
    stop()
  })

  it('stops following after teardown, even one that lands before the app mode resolves', async () => {
    const write = vi.fn(() => Promise.resolve())
    const stop = followWindowTitle(() => tString('settings.window.title'), write)
    stop()
    await settle()
    flushSync()

    void setLocale(TEST_LANG)
    flushSync()

    expect(write).not.toHaveBeenCalled()
  })
})
