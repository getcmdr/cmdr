import { describe, expect, it } from 'vitest'
import { describeUpdateFailure, formatUpdateStatus, updateFailureOffersReport } from './update-status-text'

describe('an update that isn’t a genuine Cmdr release', () => {
  it('reads like its security notice and offers a report (#457)', () => {
    expect(describeUpdateFailure({ phase: 'notGenuine' })).toBe(
      'Cmdr didn’t install this update: it isn’t a genuine Cmdr release.',
    )
    expect(updateFailureOffersReport({ phase: 'notGenuine' })).toBe(true)
  })
})

describe('an update this install can’t write into its own bundle', () => {
  const base = {
    status: 'idle',
    failure: null,
    managed: null,
    noBuild: false,
    updatedByStore: false,
    previousVersion: '0.52.0',
    nextVersion: null,
  } as const

  it('tells an administrator how to update by hand', () => {
    expect(formatUpdateStatus({ ...base, unwritable: { version: '0.54.0', blocker: 'notWritableAdminUser' } })).toBe(
      'Cmdr 0.54.0 is out, but Cmdr can’t install it itself. Download it from getcmdr.com and drag it into Applications.',
    )
  })

  it('tells a standard user their account can’t update it', () => {
    expect(formatUpdateStatus({ ...base, unwritable: { version: '0.54.0', blocker: 'notWritableStandardUser' } })).toBe(
      'Cmdr 0.54.0 is out, but your account can’t update Cmdr on this Mac.',
    )
  })

  it('says a newer Cmdr exists but not for this system yet when a newer release has no build for it, never “no updates found”', () => {
    expect(formatUpdateStatus({ ...base, unwritable: null, noBuild: true })).toBe(
      'There’s a newer Cmdr, but not for this system yet. Check again later.',
    )
  })

  it('still says “no updates found” when nothing is out', () => {
    expect(formatUpdateStatus({ ...base, unwritable: null })).toBe('No updates found. Current version: v0.52.0')
  })
})
