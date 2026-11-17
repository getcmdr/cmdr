import { describe, it, expect } from 'vitest'
import manifest from './server/settings-defaults.gen.json'
import type { SettingAdoption } from './server/settings-defaults.js'
import {
  discoveredFlags,
  dismissedFlags,
  flagReach,
  frictionFlags,
  onboardingFunnel,
  onboardingStep,
  upgradeNudgeFlag,
} from './onboarding-funnel.js'

function setting(key: string, eligible: number, onDefault: number): SettingAdoption {
  return {
    key,
    kind: 'state',
    eligible,
    onDefault,
    defaultLabel: 'off',
    latestDefaultLabel: 'off',
    defaultMovedIn: null,
    values: [],
  }
}

describe('the funnel id list', () => {
  it('names only state keys the defaults manifest knows', () => {
    const stateKeys = new Set(manifest.stateKeys)
    const ids = [onboardingStep, ...discoveredFlags, ...frictionFlags, ...dismissedFlags, upgradeNudgeFlag].map(
      (f) => f.key,
    )
    for (const id of ids) expect(stateKeys.has(id), id).toBe(true)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('gives every flag its own readable label', () => {
    const labels = [onboardingStep, ...discoveredFlags, ...frictionFlags, ...dismissedFlags, upgradeNudgeFlag].map(
      (f) => f.label,
    )
    for (const label of labels) expect(label).toMatch(/^[A-Z]/)
    expect(new Set(labels).size).toBe(labels.length)
  })
})

describe('flagReach', () => {
  it('counts installs off the default as reached, over the installs whose build has the flag', () => {
    expect(flagReach([setting('onboarding.completed', 20, 5)], onboardingStep)).toMatchObject({
      eligible: 20,
      reached: 15,
      share: 0.75,
    })
  })

  it('has no share below five eligible installs', () => {
    expect(flagReach([setting('onboarding.completed', 4, 1)], onboardingStep)).toMatchObject({
      eligible: 4,
      reached: 3,
      share: null,
    })
  })

  it('is null when no build has the flag, never a zero', () => {
    expect(flagReach([], onboardingStep)).toBeNull()
    expect(flagReach([setting('onboarding.completed', 0, 0)], onboardingStep)).toBeNull()
  })
})

describe('onboardingFunnel', () => {
  it('sorts discovered hints by share, too-few-eligible ones last, and carries the capability caveat', () => {
    const funnel = onboardingFunnel([
      setting('fileExplorer.quickFilterIntroSeen', 10, 8),
      setting('behavior.textEditorHintSeen', 10, 2),
      setting('network.firstTriggerDone', 3, 0),
      setting('behavior.adbHintDismissed', 10, 9),
    ])
    expect(funnel.discovered.map((f) => [f.key, f.capabilityGated])).toEqual([
      ['behavior.textEditorHintSeen', true],
      ['fileExplorer.quickFilterIntroSeen', false],
      ['network.firstTriggerDone', false],
    ])
    expect(funnel.dismissed.map((f) => f.key)).toEqual(['behavior.adbHintDismissed'])
    expect(funnel.friction).toEqual([])
    expect(funnel.step).toBeNull()
  })
})
