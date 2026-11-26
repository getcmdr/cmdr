/**
 * The pane's one live answer to "which volume am I on". Runes, so each case runs
 * inside `$effect.root`.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushSync } from 'svelte'
import type { VolumeInfo } from '../types'

const { resolvePathVolume } = vi.hoisted(() => ({ resolvePathVolume: vi.fn() }))
vi.mock('#lib/tauri-commands/index.js', () => ({ resolvePathVolume }))

import { createPaneVolumeState, type PaneVolumeState } from './pane-volume-state.svelte'

const root: VolumeInfo = { id: 'root', name: 'Macintosh HD', path: '/', category: 'main_volume', isEjectable: false }
const usb: VolumeInfo = { id: 'usb', name: 'USB', path: '/Volumes/USB', category: 'attached_volume', isEjectable: true }

describe('createPaneVolumeState', () => {
  let dispose: (() => void) | undefined
  let volumeId = $state('root')
  let currentPath = $state('/Users/me')

  function setup(): PaneVolumeState {
    let state!: PaneVolumeState
    dispose = $effect.root(() => {
      state = createPaneVolumeState({
        getVolumes: () => [root, usb],
        getVolumeId: () => volumeId,
        getCurrentPath: () => currentPath,
      })
    })
    flushSync()
    return state
  }

  beforeEach(() => {
    vi.clearAllMocks()
    volumeId = 'root'
    currentPath = '/Users/me'
    resolvePathVolume.mockImplementation((path: string) =>
      Promise.resolve({ volume: path.startsWith('/Volumes/USB') ? usb : root }),
    )
  })

  afterEach(() => {
    dispose?.()
  })

  it('names the drive a local pane walked into', async () => {
    const state = setup()
    currentPath = '/Volumes/USB/photos'
    flushSync()
    await vi.waitFor(() => {
      expect(state.volume?.id).toBe('usb')
    })
  })

  it('drops an answer for a path the pane has since left', async () => {
    let answerUsb!: (value: { volume: VolumeInfo }) => void
    resolvePathVolume.mockImplementationOnce(() => Promise.resolve({ volume: root }))
    const state = setup()
    resolvePathVolume.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          answerUsb = resolve
        }),
    )
    currentPath = '/Volumes/USB'
    flushSync()
    currentPath = '/Users/me/Documents'
    flushSync()
    answerUsb({ volume: usb })
    await vi.waitFor(() => {
      expect(state.containingVolumeId).toBe('root')
    })
  })

  it('has no volume for the Servers list or search results', () => {
    volumeId = 'network'
    currentPath = 'smb://'
    const state = setup()
    expect(state.volume).toBeUndefined()
    expect(resolvePathVolume).not.toHaveBeenCalled()
  })
})
