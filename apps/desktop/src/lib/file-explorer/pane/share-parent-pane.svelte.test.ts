/**
 * One pane's `..` out of an SMB share's top folder (#1238): whether the top folder
 * shows it per (default, override), where it leads, and that Go to parent does
 * nothing there while it's off.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushSync } from 'svelte'
import type { NetworkHost, VolumeInfo } from '../types'
import type { ShareParentLanding } from '../network/share-parent-row.svelte'

const { getSmbShareParent, showShareParentHintOnce, defaultShow } = vi.hoisted(() => ({
  getSmbShareParent: vi.fn(),
  showShareParentHintOnce: vi.fn(),
  defaultShow: { value: true },
}))
vi.mock('#lib/tauri-commands/index.js', () => ({ getSmbShareParent, setSmbShareParentRow: vi.fn() }))
vi.mock('#lib/settings/reactive-settings.svelte.js', () => ({ getShowShareParentRow: () => defaultShow.value }))
vi.mock('#lib/settings/index.js', () => ({ getSetting: () => defaultShow.value, setSetting: vi.fn() }))
vi.mock('../network/share-parent-hint', () => ({ showShareParentHintOnce }))

import { createShareParentPane, type ShareParentPane } from './share-parent-pane.svelte'

const host: NetworkHost = { id: 'testnas', name: 'Testnas', hostname: 'testnas.local', port: 445 }
const share = {
  id: 'smb-testnas-media',
  name: 'media on Testnas',
  path: '/Volumes/media',
  category: 'network',
  fsType: 'smbfs',
} as unknown as VolumeInfo

let dispose: (() => void) | undefined
afterEach(() => {
  dispose?.()
  dispose = undefined
})

beforeEach(() => {
  vi.clearAllMocks()
  defaultShow.value = true
})

async function create(override: boolean | null, { atTop = true } = {}) {
  getSmbShareParent.mockResolvedValue({ host, serverLabel: 'Testnas', shareName: 'media', showParentRow: override })
  const openServer = vi.fn()
  let landing: ShareParentLanding | undefined
  let pane!: ShareParentPane
  dispose = $effect.root(() => {
    pane = createShareParentPane({
      getVolume: () => share,
      getIsAtTopFolder: () => atTop,
      openServer,
      getLanding: () => landing,
      setLanding: (next) => {
        landing = next
      },
    })
  })
  flushSync()
  await vi.waitFor(() => {
    expect(getSmbShareParent).toHaveBeenCalledWith(share.id)
  })
  await Promise.resolve()
  flushSync()
  return { pane, openServer, getLanding: () => landing }
}

describe('createShareParentPane', () => {
  it('shows `..` by default, and leads to the server’s share list with the cursor on the share', async () => {
    const { pane, openServer, getLanding } = await create(null)
    expect(pane.topFolderLeadsOut).toBe(true)

    expect(pane.leaveTopFolder()).toBe(true)
    expect(openServer).toHaveBeenCalledWith(expect.objectContaining({ volumeId: 'network', networkHost: host }))
    expect(getLanding()).toEqual({
      volumeId: share.id,
      hostId: 'testnas',
      shareName: 'media',
      serverLabel: 'Testnas',
    })
  })

  it('hides `..` when the share turned it off, and Go to parent does nothing', async () => {
    const { pane, openServer } = await create(false)
    expect(pane.topFolderLeadsOut).toBe(false)
    expect(pane.leaveTopFolder()).toBe(false)
    expect(openServer).not.toHaveBeenCalled()
  })

  it('shows `..` on a share turned on while the default is off', async () => {
    defaultShow.value = false
    const { pane } = await create(true)
    expect(pane.topFolderLeadsOut).toBe(true)
  })

  it('hides `..` with the default off and no override', async () => {
    defaultShow.value = false
    const { pane } = await create(null)
    expect(pane.topFolderLeadsOut).toBe(false)
  })

  it('leaves a folder inside the share to the ordinary parent navigation', async () => {
    const { pane, openServer } = await create(null, { atTop: false })
    expect(pane.leaveTopFolder()).toBe(false)
    expect(openServer).not.toHaveBeenCalled()
  })

  it('spends the landing once the share list is on screen, raising the explainer', async () => {
    const { pane, getLanding } = await create(null)
    pane.leaveTopFolder()
    pane.landed()
    expect(showShareParentHintOnce).toHaveBeenCalledWith(expect.objectContaining({ shareName: 'media' }))
    expect(getLanding()).toBeUndefined()
  })
})
