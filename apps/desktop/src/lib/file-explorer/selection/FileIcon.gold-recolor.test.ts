/**
 * Tests for the Cmdr-gold folder recolor predicate in `FileIcon.svelte`.
 *
 * When the app color is "Cmdr gold", generic folder icons get a grayscale-first
 * CSS filter (`.gold-folder`) that re-tints them gold regardless of the macOS
 * system accent. The subtle contract this pins:
 *   - special system folders (`special:*`) ARE recolored where the OS draws them
 *     accent-tinted (macOS), so Downloads/Desktop/etc. don't leak the system accent
 *     through, and keep their own colors where it doesn't (Windows, #1479),
 *   - packages (`pkg:*`, full-color `.app` icons) are NEVER recolored, since the
 *     filter would ruin them. Which custom-icon folders recolor is pinned in
 *     `FileIcon.custom-folder-icon.test.ts`.
 */
import { describe, it, expect, vi, afterEach } from 'vitest'
import { mount, unmount, flushSync } from 'svelte'

// Every id resolves to a data URL so the live `<img>` (not the fallback) renders.
vi.mock('#lib/icon-cache.js', async () => {
  const { writable } = await import('svelte/store')
  return {
    getCachedIcon: () => '/icons/stub.svg',
    // No folder here carries a Finder custom icon, so the recolor gate depends
    // solely on iconId. The custom-icon case is pinned in
    // `FileIcon.custom-folder-icon.test.ts`.
    getCachedCustomFolderIcon: () => undefined,
    isAccentTintedCustomFolder: () => false,
    iconCacheVersion: writable(0),
  }
})

// Force the "Cmdr gold" app color on, so the recolor gate depends solely on iconId.
vi.mock('#lib/settings/reactive-settings.svelte.js', () => ({
  getFileSizeFormat: () => 'binary',
  getIsCmdrGold: () => true,
}))

import FileIcon from './FileIcon.svelte'
import { withConventions } from '#lib/platform/test-platform.js'

const baseEntry = {
  name: 'entry',
  path: '/Users/test/entry',
  isDirectory: true,
  isSymlink: false,
  size: 0,
  modifiedAt: 1710000000,
  iconId: 'dir',
  permissions: 420,
  owner: 'test',
  group: 'staff',
}

let target: HTMLElement | undefined
let app: ReturnType<typeof mount> | undefined

function renderWithIconId(iconId: string): HTMLImageElement {
  target = document.createElement('div')
  document.body.appendChild(target)
  app = mount(FileIcon, { target, props: { file: { ...baseEntry, iconId } } })
  flushSync()
  const img = target.querySelector('img.icon')
  if (!img) throw new Error(`no icon img rendered for iconId=${iconId}`)
  return img as HTMLImageElement
}

afterEach(() => {
  if (app) void unmount(app)
  app = undefined
  target?.remove()
  target = undefined
})

describe('FileIcon gold recolor gate', () => {
  it.each(['dir', 'symlink-dir', 'special:downloads', 'special:home'])(
    'recolors folder icon id %s to gold',
    (iconId) => {
      expect(renderWithIconId(iconId).classList.contains('gold-folder')).toBe(true)
    },
  )

  it.each(['pkg:/Applications/Safari.app', 'path:/Users/test/CustomFolder', 'ext:md', 'file'])(
    'leaves icon id %s untouched (no gold recolor)',
    (iconId) => {
      expect(renderWithIconId(iconId).classList.contains('gold-folder')).toBe(false)
    },
  )
})

// #1479: Windows draws its standard folders as full-color artwork, which the filter grayed into
// plain gold folders, so there only the generic folder is recolored.
describe('FileIcon gold recolor where standard folders keep their own colors', () => {
  let undo: (() => void) | undefined
  afterEach(() => {
    undo?.()
    undo = undefined
  })

  it.each(['special:downloads', 'special:desktop'])('leaves %s in its own colors on Windows', (iconId) => {
    undo = withConventions('windows')
    expect(renderWithIconId(iconId).classList.contains('gold-folder')).toBe(false)
  })

  it.each(['dir', 'symlink-dir'])('still recolors the generic folder %s on Windows', (iconId) => {
    undo = withConventions('windows')
    expect(renderWithIconId(iconId).classList.contains('gold-folder')).toBe(true)
  })
})
