import { describe, expect, it, vi } from 'vitest'
import type { NameRules } from '#lib/ipc/bindings.js'
import { POSIX_RULES } from '#lib/utils/name-rules.js'

const { getNameRulesSpy } = vi.hoisted(() => ({ getNameRulesSpy: vi.fn() }))
vi.mock('#lib/tauri-commands/index.js', () => ({ getNameRules: getNameRulesSpy }))

import { createFolderNameRules } from './folder-name-rules'
import type { createRenameState, RenameSessionId } from './rename-state.svelte'

/** A rename state where only `superseded` sessions are stale. */
function renameState(superseded: Set<RenameSessionId>): ReturnType<typeof createRenameState> {
  return {
    active: true,
    isSuperseded: (id: RenameSessionId) => superseded.has(id),
    hasChanged: () => true,
  } as unknown as ReturnType<typeof createRenameState>
}

const session = (n: number): RenameSessionId => n

const STICK: NameRules = {
  format: 'windows',
  windowsCharacters: 'warn',
  reservedNames: 'warn',
  nameLimit: { max: 255, unit: 'utf16Units' },
  pathLimit: { max: 1023, unit: 'bytes' },
}

describe('createFolderNameRules', () => {
  it('starts at posix and takes the folder`s answer, asking for that folder on that volume', async () => {
    getNameRulesSpy.mockResolvedValueOnce(STICK)
    const rules = createFolderNameRules(() => 'usb-stick', renameState(new Set()))
    const onChange = vi.fn()

    rules.ask('/Volumes/STICK/photos', session(1), onChange)
    expect(rules.current).toEqual(POSIX_RULES)
    await vi.waitFor(() => {
      expect(onChange).toHaveBeenCalledOnce()
    })

    expect(rules.current).toEqual(STICK)
    expect(getNameRulesSpy).toHaveBeenCalledWith('usb-stick', '/Volumes/STICK/photos')
  })

  it('drops an answer for a session that was superseded', async () => {
    let answer: (rules: NameRules) => void = () => {}
    getNameRulesSpy.mockReturnValueOnce(new Promise((resolve) => (answer = resolve)))
    const superseded = new Set<RenameSessionId>()
    const rules = createFolderNameRules(() => 'root', renameState(superseded))
    const onChange = vi.fn()

    rules.ask('/Users/test', session(1), onChange)
    superseded.add(session(1))
    answer({ ...STICK, format: 'smbShare' })
    await Promise.resolve()

    expect(onChange).not.toHaveBeenCalled()
    expect(rules.current).toEqual(POSIX_RULES)
  })
})
