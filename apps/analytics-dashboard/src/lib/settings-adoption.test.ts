import { describe, it, expect } from 'vitest'
import type { SettingAdoption } from './server/settings-defaults.js'
import { preferencesOnly, topMoved } from './settings-adoption.js'

function setting(key: string, onDefault: number, kind: SettingAdoption['kind'] = 'preference'): SettingAdoption {
  return {
    key,
    kind,
    eligible: 100,
    onDefault,
    defaultLabel: 'off',
    latestDefaultLabel: 'off',
    defaultMovedIn: null,
    values: [],
  }
}

describe('preferencesOnly', () => {
  it('drops remembered UI and onboarding state', () => {
    const settings = [setting('theme.mode', 90), setting('onboarding.completed', 10, 'state')]
    expect(preferencesOnly(settings).map((s) => s.key)).toEqual(['theme.mode'])
  })
})

describe('topMoved', () => {
  it('keeps the most-changed n, unmoved settings left out', () => {
    const settings = Array.from({ length: 20 }, (_, i) => setting(`s${String(i).padStart(2, '0')}`, 100 - i))
    const top = topMoved(settings)
    expect(top).toHaveLength(15)
    expect(top[0].key).toBe('s19')
    expect(top.some((s) => s.key === 's00')).toBe(false)
    expect(topMoved(settings, 3).map((s) => s.key)).toEqual(['s19', 's18', 's17'])
  })
})
