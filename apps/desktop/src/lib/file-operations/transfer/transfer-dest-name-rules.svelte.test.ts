/**
 * The transfer dialog's destination rules answer only for the path they were asked about: a
 * failed lookup, an emptied path, or a path still waiting for its answer reads as `POSIX_RULES`,
 * never as the previous destination's rules.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync } from 'svelte'
import type { NameRules } from '#lib/ipc/bindings.js'
import { POSIX_RULES } from '#lib/utils/name-rules.js'

const getNameRules = vi.fn<(volumeId: string, path: string) => Promise<NameRules>>()
vi.mock('#lib/tauri-commands/index.js', () => ({
  getNameRules: (volumeId: string, path: string) => getNameRules(volumeId, path),
}))

const { createTransferDestNameRules } = await import('./transfer-dest-name-rules.svelte')

const EXFAT: NameRules = {
  format: 'windows',
  windowsCharacters: 'warn',
  reservedNames: 'warn',
  nameLimit: { max: 255, unit: 'utf16Units' },
  pathLimit: { max: 1023, unit: 'bytes' },
}

let cleanup: (() => void) | null = null

beforeEach(() => {
  vi.useFakeTimers()
  getNameRules.mockReset()
})

afterEach(() => {
  cleanup?.()
  cleanup = null
  vi.useRealTimers()
})

function track(initialPath: string) {
  const deps = $state({ path: initialPath })
  let rules!: ReturnType<typeof createTransferDestNameRules>
  cleanup = $effect.root(() => {
    rules = createTransferDestNameRules({
      getVolumeId: () => 'root',
      getPath: () => deps.path,
      getDestroyed: () => false,
    })
  })
  flushSync()
  return { deps, rules }
}

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(300)
}

describe('the transfer dialog destination rules', () => {
  it('answers for the stick once it answers', async () => {
    getNameRules.mockResolvedValue(EXFAT)
    const { rules } = track('/Volumes/STICK')
    await settle()
    expect(rules.current).toEqual(EXFAT)
    expect(rules.limits).toBeDefined()
  })

  it('falls back to POSIX when the next path’s lookup fails', async () => {
    getNameRules.mockResolvedValueOnce(EXFAT).mockRejectedValueOnce(new Error('timed out'))
    const { deps, rules } = track('/Volumes/STICK')
    await settle()
    deps.path = '/Volumes/GONE'
    flushSync()
    await settle()
    expect(rules.current).toEqual(POSIX_RULES)
    expect(rules.limits).toBeUndefined()
  })

  it('falls back to POSIX when the path is emptied', async () => {
    getNameRules.mockResolvedValue(EXFAT)
    const { deps, rules } = track('/Volumes/STICK')
    await settle()
    deps.path = ''
    flushSync()
    expect(rules.current).toEqual(POSIX_RULES)
  })

  it('never answers a new path with the old one’s rules while its own is on the way', async () => {
    let answerHome!: (rules: NameRules) => void
    getNameRules.mockResolvedValueOnce(EXFAT).mockReturnValueOnce(
      new Promise<NameRules>((resolve) => {
        answerHome = resolve
      }),
    )
    const { deps, rules } = track('/Volumes/STICK')
    await settle()
    deps.path = '/Users/test'
    flushSync()
    await settle()
    expect(rules.current).toEqual(POSIX_RULES)
    answerHome(POSIX_RULES)
    await settle()
    expect(rules.current).toEqual(POSIX_RULES)
  })
})
