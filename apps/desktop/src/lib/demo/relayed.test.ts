import { afterEach, describe, expect, it } from 'vitest'
import type { EventTarget } from './backend.js'
import { DEMO_HOME } from './demo-tree.js'
import { demoFavorites, resetDemoFavorites } from './favorites.js'
import { RELAYED_COMMANDS, relayedAnswers } from './relayed.js'
import { volumeAnswers } from './volumes.js'

function setup() {
  const emitted: { event: string; payload: unknown; target: EventTarget }[] = []
  const answers = relayedAnswers((event, payload, target) => emitted.push({ event, payload, target })) as Record<
    string,
    (payload: Record<string, unknown>) => unknown
  >
  return { answers, emitted }
}

const names = () => demoFavorites().map((favorite) => favorite.name)

afterEach(resetDemoFavorites)

describe('relayed answers', () => {
  it('answers exactly the commands it claims', () => {
    expect(Object.keys(setup().answers).sort()).toEqual([...RELAYED_COMMANDS].sort())
  })

  it('adds a favorite named after its folder, lists it, and tells every window the list changed', () => {
    const { answers, emitted } = setup()
    answers.addFavorite({ path: `${DEMO_HOME}/Documents/Taxes`, name: null })
    expect(names()).toEqual(['you', 'Desktop', 'Documents', 'Downloads', 'Taxes'])
    const listed = volumeAnswers.listVolumes().data.map((location) => location.name)
    expect(listed).toContain('Taxes')
    expect(emitted).toHaveLength(1)
    expect(emitted[0].event).toBe('volumes-changed')
    expect(emitted[0].target).toEqual({ kind: 'everyone' })
    expect((emitted[0].payload as { data: { name: string }[] }).data.map((l) => l.name)).toContain('Taxes')
  })

  it('moves a re-added folder to the end, keeping its id, as the desktop store does', () => {
    const { answers } = setup()
    answers.addFavorite({ path: `${DEMO_HOME}/Desktop`, name: 'Desk' })
    expect(names()).toEqual(['you', 'Documents', 'Downloads', 'Desk'])
    expect(demoFavorites().at(-1)?.id).toBe('fav-desktop')
  })

  it('renames, removes, and reorders, never dropping an entry the order left out', () => {
    const { answers } = setup()
    answers.renameFavorite({ id: 'fav-home', name: 'Home' })
    answers.removeFavorite({ id: 'fav-desktop' })
    answers.reorderFavorites({ orderedIds: ['fav-downloads', 'nope'] })
    expect(names()).toEqual(['Downloads', 'Home', 'Documents'])
  })

  it('gives a letter one owner and refuses a non-letter', () => {
    const { answers } = setup()
    answers.setFavoriteShortcut({ id: 'fav-documents', shortcut: 'h' })
    const shortcuts = Object.fromEntries(demoFavorites().map((f) => [f.id, f.shortcut]))
    expect(shortcuts['fav-documents']).toBe('H')
    expect(shortcuts['fav-home']).toBeNull()
    expect(() => answers.setFavoriteShortcut({ id: 'fav-home', shortcut: '1' })).toThrow()
    expect(() => answers.setFavoriteShortcut({ id: 'fav-home', shortcut: '1' })).toThrowError(
      expect.objectContaining({ type: 'invalidLetter' }) as Error,
    )
  })

  it('hands a restricted window’s setting to the main window to save, as the desktop forwards it', () => {
    const { answers, emitted } = setup()
    expect(answers.persistRestrictedWindowSetting({ setting: 'viewerWordWrap', value: true })).toBeNull()
    expect(emitted).toEqual([
      {
        event: 'persist-restricted-setting',
        payload: { id: 'viewer.wordWrap', value: true },
        target: { kind: 'label', label: 'main' },
      },
    ])
  })
})
