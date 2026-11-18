import { describe, expect, it } from 'vitest'
import type { NamePortabilityTally, NameRules } from '#lib/ipc/bindings.js'
import { POSIX_RULES } from '#lib/utils/name-rules.js'
import { namePortabilitySummary } from './transfer-name-portability'

const tally: NamePortabilityTally = {
  windowsCharacters: { count: 12, examples: ['12:30 notes.txt'] },
  reservedNames: { count: 1, examples: ['con.txt'] },
  endsWithSpaceOrPeriod: { count: 0, examples: [] },
}
const stickOnMac: NameRules = {
  format: 'windows',
  windowsCharacters: 'warn',
  reservedNames: 'warn',
  nameLimit: { max: 255, unit: 'utf16Units' },
  pathLimit: null,
}
const share: NameRules = { ...stickOnMac, format: 'smbShare', nameLimit: { max: 255, unit: 'bytes' } }

describe('namePortabilitySummary', () => {
  it('says nothing for a POSIX destination, or before the scan answers', () => {
    expect(namePortabilitySummary(tally, POSIX_RULES)).toBeNull()
    expect(namePortabilitySummary(null, stickOnMac)).toBeNull()
  })

  it('counts every warned class, and names Linux for a stick’s stand-ins', () => {
    expect(namePortabilitySummary(tally, stickOnMac)).toEqual({
      count: 13,
      alsoLinux: true,
      example: '12:30 notes.txt',
    })
  })

  it('leaves Linux out on a share', () => {
    expect(namePortabilitySummary(tally, share)?.alsoLinux).toBe(false)
  })

  it('leaves out what the destination refuses: the landing asks about those per item', () => {
    const stickOnLinux: NameRules = { ...stickOnMac, windowsCharacters: 'refuse' }
    expect(namePortabilitySummary(tally, stickOnLinux)).toEqual({ count: 1, alsoLinux: false, example: 'con.txt' })
  })
})
