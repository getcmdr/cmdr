/**
 * A setting's description placement as the registry authors it (`descriptionInTip`) and as the
 * resolved definition reports it (`descriptionPlacement`, `summary`).
 */

import { describe, it, expect } from 'vitest'
import { resolveDefinition } from './settings-resolve'
import { settingsRegistry } from './settings-registry'
import type { SettingDefinitionSource } from './types'

const base: SettingDefinitionSource = {
  id: 'theme.mode',
  section: ['Appearance', 'Colors and formats'],
  labelKey: 'settings.theme.mode.label',
  descriptionKey: 'settings.fileExplorer.git.showVirtualGitPortal.description',
  type: 'enum',
  default: 'system',
}

describe('resolveDefinition description placement', () => {
  it('keeps the description inline when the setting doesn’t opt in', () => {
    const def = resolveDefinition(base)
    expect(def.descriptionPlacement).toBe('inline')
    expect(def.summary).toBe('')
    expect(def.description).toContain('.git')
  })

  it('moves the description behind the tip with `descriptionInTip: true`', () => {
    const def = resolveDefinition({ ...base, descriptionInTip: true })
    expect(def.descriptionPlacement).toBe('tip')
    expect(def.summary).toBe('')
    // The tip carries the same full text the inline description did.
    expect(def.description).toBe(resolveDefinition(base).description)
  })

  it('resolves a summary line for a tip that has one', () => {
    const def = resolveDefinition({ ...base, descriptionInTip: { summaryKey: 'settings.theme.mode.label' } })
    expect(def.descriptionPlacement).toBe('tip')
    expect(def.summary).toBe('Theme mode')
  })
})

describe('the registry’s tip settings', () => {
  // A split setting's tip may be empty on one OS (`network.enabled` has nothing to add on Linux); its summary still shows.
  it('give every tip something to show', () => {
    const empty = settingsRegistry
      .filter((s) => s.descriptionPlacement === 'tip' && s.description === '' && s.summary === '')
      .map((s) => s.id)
    expect(empty).toEqual([])
  })
})
