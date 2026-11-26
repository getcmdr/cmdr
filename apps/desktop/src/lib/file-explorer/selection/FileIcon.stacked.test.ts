/**
 * Tests for stacked icon ids in `FileIcon.svelte`: a server's share (`network:share`) draws
 * its base folder icon (`dir`) twice, and every folder rule (the cache lookup, the Cmdr-gold
 * tint) runs on that base, so it looks like the platform's folder on every OS.
 */
import { describe, it, expect, vi, afterEach } from 'vitest'
import { mount, unmount, flushSync } from 'svelte'

const gold = vi.hoisted(() => ({ on: false }))

// Only `dir` is cached: a share drawing anything else would show its stand-in instead.
vi.mock('#lib/icon-cache.js', async () => {
  const { writable } = await import('svelte/store')
  return {
    getCachedIcon: (iconId: string) => (iconId === 'dir' ? '/icons/dir.png' : undefined),
    getCachedCustomFolderIcon: () => '/icons/custom.png',
    isAccentTintedCustomFolder: () => false,
    iconCacheVersion: writable(0),
  }
})

vi.mock('#lib/settings/reactive-settings.svelte.js', () => ({
  getFileSizeFormat: () => 'binary',
  getIsCmdrGold: () => gold.on,
}))

import FileIcon from './FileIcon.svelte'

const share = {
  name: 'archive',
  path: 'smb://testnas/archive',
  isDirectory: true,
  isSymlink: false,
  iconId: 'network:share',
  permissions: 0,
  owner: '',
  group: '',
}

let target: HTMLElement | undefined
let app: ReturnType<typeof mount> | undefined

function render(iconId = share.iconId): HTMLElement {
  target = document.createElement('div')
  document.body.appendChild(target)
  app = mount(FileIcon, { target, props: { file: { ...share, iconId } } })
  flushSync()
  return target
}

afterEach(() => {
  if (app) void unmount(app)
  app = undefined
  target?.remove()
  target = undefined
  gold.on = false
})

describe('FileIcon stacked ids', () => {
  it('draws a share as the cached folder icon twice, back and front', () => {
    const images = [...render().querySelectorAll('img')]
    expect(images.map((img) => img.className.split(' ').find((c) => c.startsWith('stacked-')))).toEqual([
      'stacked-back',
      'stacked-front',
    ])
    expect(images.every((img) => img.getAttribute('src') === '/icons/dir.png')).toBe(true)
  })

  it('never draws a Finder custom icon on a share: only a real folder has one', () => {
    const images = [...render().querySelectorAll('img')]
    expect(images.some((img) => img.getAttribute('src') === '/icons/custom.png')).toBe(false)
  })

  it('is decorative, like every row icon: the row says its name', () => {
    const images = [...render().querySelectorAll('img')]
    expect(images.every((img) => img.getAttribute('alt') === '')).toBe(true)
  })

  it('tints both copies gold under the Cmdr-gold folder setting, as a folder', () => {
    gold.on = true
    const images = [...render().querySelectorAll('img')]
    expect(images).toHaveLength(2)
    expect(images.every((img) => img.classList.contains('gold-folder'))).toBe(true)
  })

  it('draws an ordinary folder once', () => {
    expect(render('dir').querySelectorAll('img')).toHaveLength(1)
  })
})
