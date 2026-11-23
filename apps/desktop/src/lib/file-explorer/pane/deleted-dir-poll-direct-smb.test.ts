/**
 * Pins getcmdr/cmdr#1338: a pane on a direct SMB session bounced to the
 * parent ~3 s after entering a folder whose name the macOS kernel mount can't see (an NFD
 * name on the server, common on a Synology). The listing came from the direct session, but
 * the poll asked the BOOT DISK (no volume id), which reached the share through smbfs and
 * got ENOENT twice. The poll has to ask the pane's own volume, the one that listed it.
 *
 * Repro and analysis: `pane/DETAILS.md` § `paneFolderIsPolledForDeletion`.
 */
import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest'

const { ipc, resolution } = vi.hoisted<{
  ipc: { pathExistsChecked: Mock }
  resolution: { resolveValidPath: Mock }
}>(() => ({
  ipc: { pathExistsChecked: vi.fn() },
  resolution: { resolveValidPath: vi.fn() },
}))

vi.mock('#lib/tauri-commands/index.js', () => ({ pathExistsChecked: ipc.pathExistsChecked }))
vi.mock('../navigation/path-resolution', () => ({ resolveValidPath: resolution.resolveValidPath }))

import { createDeletedDirPoll, type DeletedDirPollDeps } from './deleted-dir-poll'

const SMB_VOLUME_ID = 'smb-testnas-445-public-0000000000000000'
const SHARE_ROOT = '/Volumes/public'
// `nfd-cafe` + U+0301: the server's own bytes, which smbfs never lists.
const NFD_FOLDER = `${SHARE_ROOT}/nfd-café`

describe('createDeletedDirPoll on a direct SMB pane (#1338)', () => {
  let navigateToFallback: Mock

  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
    navigateToFallback = vi.fn()
    resolution.resolveValidPath.mockResolvedValue(SHARE_ROOT)
    // The kernel mount (no volume id) misses the folder; the direct session (its id) has it.
    ipc.pathExistsChecked.mockImplementation((path: string, volumeId?: string) =>
      Promise.resolve({ data: path === SHARE_ROOT || volumeId === SMB_VOLUME_ID, timedOut: false }),
    )
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('keeps the pane in a folder the direct session listed, whatever the kernel mount says', async () => {
    const deps = {
      getListingId: () => 'listing-1',
      getLoading: () => false,
      getFolderIsPolled: () => true,
      getCurrentPath: () => NFD_FOLDER,
      getVolumePath: () => SHARE_ROOT,
      getVolumeId: () => SMB_VOLUME_ID,
      getConnectionState: () => 'direct',
      navigateToFallback,
    } satisfies DeletedDirPollDeps
    const poll = createDeletedDirPoll(deps)
    poll.start()

    await vi.advanceTimersByTimeAsync(10_000)

    expect(ipc.pathExistsChecked).toHaveBeenCalledWith(NFD_FOLDER, SMB_VOLUME_ID)
    expect(navigateToFallback).not.toHaveBeenCalled()
    poll.stop()
  })
})
