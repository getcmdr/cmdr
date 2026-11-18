import { describe, expect, it } from 'vitest'
import { installsByOs, reachByDay, splitActives } from './active-installs.js'
import type { UpdateActivityRow } from './server/sources/cloudflare.js'

function updates(rows: Array<[string, string, number]>): UpdateActivityRow[] {
  return rows.map(([day, version, updaters]) => ({ day, version, updaters }))
}

describe('reachByDay', () => {
  it('sums the versions of a day and reads unknown for a day the update data misses', () => {
    const rows = updates([
      ['2026-08-01', '0.24.0', 40],
      ['2026-08-01', '0.25.0', 10],
      ['2026-08-03', '0.25.0', 7],
    ])
    expect(reachByDay(rows, ['2026-08-01', '2026-08-02', '2026-08-03'])).toEqual([50, null, 7])
  })
})

describe('splitActives', () => {
  it('splits each day into new and returning installs', () => {
    expect(
      splitActives([
        { dau: 10, newInstalls: 3 },
        { dau: 0, newInstalls: 0 },
      ]),
    ).toEqual({
      newInstalls: [3, 0],
      returning: [7, 0],
    })
  })

  it('never draws a negative returning count when a swept install leaves more new than active', () => {
    expect(splitActives([{ dau: 2, newInstalls: 3 }]).returning).toEqual([0])
  })
})

describe('installsByOs', () => {
  it('names each OS the way people write it, largest first', () => {
    expect(installsByOs({ windows: 2, macos: 40, linux: 2 })).toEqual([
      { label: 'macOS', count: 40 },
      { label: 'Linux', count: 2 },
      { label: 'Windows', count: 2 },
    ])
  })

  it('lists nothing for a day from a Worker older than the split', () => {
    expect(installsByOs(undefined)).toEqual([])
  })
})
