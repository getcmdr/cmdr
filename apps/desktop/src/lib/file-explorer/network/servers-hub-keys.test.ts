import { describe, it, expect } from 'vitest'
import { cursorAcrossRebuild } from './servers-hub-keys'
import { NEARBY_GROUP_ID, type HubItem } from './servers-hub-items'
import type { HubRow } from './servers-hub-rows'

/** A saved server's row: `a`. A nearby one: `~a`. The group's header: `>` open, `<` collapsed. */
function items(...specs: string[]): HubItem[] {
  return specs.map((spec): HubItem => {
    if (spec === '>' || spec === '<') {
      return { kind: 'nearby_group', id: NEARBY_GROUP_ID, count: 2, expanded: spec === '>' }
    }
    const inNearbyGroup = spec.startsWith('~')
    const id = inNearbyGroup ? spec.slice(1) : spec
    return { kind: 'row', id, row: { id } as HubRow, inNearbyGroup }
  })
}

describe('cursorAcrossRebuild', () => {
  it('follows the row it was on when the list re-sorts', () => {
    expect(cursorAcrossRebuild(items('a', 'b', 'c'), items('x', 'a', 'b', 'c'), 1)).toBe(2)
  })

  it('keeps the add row the add row', () => {
    expect(cursorAcrossRebuild(items('a', 'b'), items('a', 'b', 'c'), 2)).toBe(3)
  })

  it('lands on the first row when the list fills, not on the add row an empty list had', () => {
    expect(cursorAcrossRebuild(items(), items('a', 'b'), 0)).toBe(0)
  })

  it('clamps when its row left', () => {
    expect(cursorAcrossRebuild(items('a', 'b', 'c'), items('a'), 2)).toBe(1)
  })

  it('lands on the first server, not on the header, when a list of only nearby servers fills', () => {
    expect(cursorAcrossRebuild(items(), items('>', '~a', '~b'), 0)).toBe(1)
  })

  it('rests on the header when a list that fills has nothing else to rest on', () => {
    expect(cursorAcrossRebuild(items(), items('<'), 0)).toBe(0)
  })

  it('moves to the header when the group collapses over the cursor, never to a hidden row', () => {
    expect(cursorAcrossRebuild(items('a', '>', '~b', '~c'), items('a', '<'), 3)).toBe(1)
  })

  it('stays on the header across a toggle', () => {
    expect(cursorAcrossRebuild(items('a', '<'), items('a', '>', '~b', '~c'), 1)).toBe(1)
    expect(cursorAcrossRebuild(items('a', '>', '~b', '~c'), items('a', '<'), 1)).toBe(1)
  })

  it('keeps the add row the add row across a toggle', () => {
    expect(cursorAcrossRebuild(items('a', '>', '~b', '~c'), items('a', '<'), 4)).toBe(2)
  })

  it('follows a nearby server that gets saved while the group collapses', () => {
    expect(cursorAcrossRebuild(items('>', '~a', '~b'), items('a', '<'), 1)).toBe(0)
  })
})

describe('cursorAcrossRebuild as a server’s share list lands (#1404)', () => {
  /** A share item under the saved server `nas`: an unsaved share, or the More row. */
  const shareItem = (id: string): HubItem => ({ kind: 'more_shares', id, server: { id: 'nas' } as HubRow })

  it('stays on the same item when share rows appear above it', () => {
    const before = [...items('nas'), shareItem('more-shares:nas'), ...items('attic')]
    const after = [
      ...items('nas'),
      shareItem('unsaved-share:nas:a'),
      shareItem('unsaved-share:nas:b'),
      ...items('attic'),
    ]
    expect(cursorAcrossRebuild(before, after, 2)).toBe(3)
  })

  it('keeps the add row the add row when share rows appear', () => {
    const after = [...items('nas'), shareItem('unsaved-share:nas:a'), ...items('attic')]
    expect(cursorAcrossRebuild(items('nas', 'attic'), after, 2)).toBe(3)
  })
})
