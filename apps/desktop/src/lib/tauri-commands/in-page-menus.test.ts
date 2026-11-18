import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { BarMenuView, MenuBarAnswer, MenuBarChanged } from '#lib/ipc/bindings.js'

const ipc = vi.hoisted(() => ({
  answers: [] as (MenuBarAnswer | Error)[],
  listener: null as ((event: { payload: MenuBarChanged }) => void) | null,
  asked: 0,
  /** Lets a test land events while `get_menu_bar` is in flight. */
  beforeAnswer: null as (() => void) | null,
}))

vi.mock('#lib/ipc/bindings.js', () => ({
  commands: {
    getMenuBar: () => {
      ipc.asked++
      ipc.beforeAnswer?.()
      ipc.beforeAnswer = null
      const answer = ipc.answers.shift()
      if (!answer) throw new Error('no answer queued')
      return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer)
    },
  },
  events: {
    menuBarChanged: {
      listen: (handler: (event: { payload: MenuBarChanged }) => void) => {
        ipc.listener = handler
        return Promise.resolve(() => {
          ipc.listener = null
        })
      },
    },
  },
}))

import { applyMenuBarChange } from '#lib/ui/command-menu/bar-change.js'
import { followMenuBar } from './in-page-menus'

function bar(hidden: boolean, label = 'Show hidden files'): BarMenuView[] {
  return [
    {
      id: 'menu_view',
      title: { text: 'View' },
      rows: [
        {
          kind: 'submenu',
          id: 'menu.view.more',
          label: { text: 'More' },
          enabled: true,
          rows: [
            {
              kind: 'check',
              id: 'show_hidden_files',
              label: { text: label },
              shortcut: null,
              ariaKeyshortcuts: null,
              enabled: true,
              checked: hidden,
              checkKind: 'checkbox',
            },
          ],
        },
      ],
    },
  ]
}

function hiddenFlip(revision: number, checked: boolean): MenuBarChanged {
  return {
    revision,
    menus: null,
    rows: [
      {
        id: 'show_hidden_files',
        label: { text: 'Show hidden files' },
        shortcut: null,
        ariaKeyshortcuts: null,
        enabled: true,
        checked,
      },
    ],
  }
}

const checkedIn = (menus: BarMenuView[]): boolean => {
  const sub = menus[0].rows[0]
  const row = sub.kind === 'submenu' ? sub.rows[0] : undefined
  return row?.kind === 'check' ? row.checked : false
}

beforeEach(() => {
  ipc.answers = []
  ipc.listener = null
  ipc.asked = 0
  ipc.beforeAnswer = null
})

afterEach(() => {
  vi.useRealTimers()
})

describe('applyMenuBarChange', () => {
  it('moves a row by id, inside a submenu too', () => {
    expect(checkedIn(applyMenuBarChange(bar(false), hiddenFlip(2, true)))).toBe(true)
  })

  it('takes the whole bar when its shape moved', () => {
    const fresh = bar(true, 'Other')
    expect(applyMenuBarChange(bar(false), { revision: 2, rows: [], menus: fresh })).toBe(fresh)
  })
})

describe('followMenuBar', () => {
  it('subscribes before it asks, so no change between the two is lost', async () => {
    ipc.answers.push({ revision: 1, menus: bar(false) })
    // The change lands while the answer is in flight: one past it, so it applies on top.
    ipc.beforeAnswer = () => ipc.listener?.({ payload: hiddenFlip(2, true) })
    const seen: boolean[] = []
    await followMenuBar((menus) => seen.push(checkedIn(menus)))
    expect(seen).toEqual([false, true])
  })

  it('drops a change the answer already holds', async () => {
    ipc.answers.push({ revision: 5, menus: bar(true) })
    ipc.beforeAnswer = () => ipc.listener?.({ payload: hiddenFlip(5, true) })
    const seen: boolean[] = []
    await followMenuBar((menus) => seen.push(checkedIn(menus)))
    ipc.listener?.({ payload: hiddenFlip(4, false) })
    expect(seen).toEqual([true])
  })

  it('applies the next revision, and asks again on a gap', async () => {
    ipc.answers.push({ revision: 1, menus: bar(false) })
    const seen: boolean[] = []
    await followMenuBar((menus) => seen.push(checkedIn(menus)))
    ipc.listener?.({ payload: hiddenFlip(2, true) })
    ipc.answers.push({ revision: 9, menus: bar(false) })
    ipc.listener?.({ payload: hiddenFlip(4, true) })
    await Promise.resolve()
    await Promise.resolve()
    expect(ipc.asked).toBe(2)
    expect(seen).toEqual([false, true, false])
  })

  it('retries a failed ask, keeping only the changes newer than the answer it waits for', async () => {
    vi.useFakeTimers()
    ipc.answers.push(new Error('the backend is still starting'))
    const seen: boolean[] = []
    const stop = await followMenuBar((menus) => seen.push(checkedIn(menus)))
    expect(seen).toEqual([])
    // Changes while it has no answer wait for the retry's, which already holds them.
    for (let revision = 2; revision < 50; revision++) ipc.listener?.({ payload: hiddenFlip(revision, true) })
    ipc.answers.push({ revision: 49, menus: bar(true) })
    ipc.beforeAnswer = () => ipc.listener?.({ payload: hiddenFlip(50, false) })
    await vi.runOnlyPendingTimersAsync()
    expect(ipc.asked).toBe(2)
    expect(seen).toEqual([true, false])
    stop()
  })

  it('hands back the unsubscribe when the first ask fails, and stops retrying once called', async () => {
    vi.useFakeTimers()
    ipc.answers.push(new Error('the backend is still starting'))
    const stop = await followMenuBar(() => {})
    expect(ipc.listener).not.toBeNull()
    stop()
    expect(ipc.listener).toBeNull()
    await vi.runAllTimersAsync()
    expect(ipc.asked).toBe(1)
  })

  it('stops following once unsubscribed', async () => {
    ipc.answers.push({ revision: 1, menus: bar(false) })
    const stop = await followMenuBar(() => {})
    stop()
    expect(ipc.listener).toBeNull()
  })
})
