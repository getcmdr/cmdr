/**
 * Client-safe onboarding funnel: how far installs get through onboarding and the once-ever hints, read
 * from the state flags in settings adoption (each install's latest heartbeat config shape).
 *
 * - **An explicit id list**, never a pattern over setting names: which flags exist and what they mean is
 *   decided here, and the test pins every id as a `kind: 'state'` key in the defaults manifest.
 * - **Reached = moved off its default.** Every flag here defaults to "not yet" (`false`), so an install
 *   that's off the default has seen, done, or dismissed the thing.
 * - **Eligible = installs whose build has the flag.** A build without it is out of the denominator, never a
 *   zero. Below `minEligible` there's no share, only "n of m".
 * - Left out on purpose: `advanced.oldMacosNoticeShown` (only old macOS sees it, so it says nothing about
 *   onboarding).
 */
import type { SettingAdoption } from './server/settings-defaults.js'

/** One flag in the funnel. */
export interface FunnelFlag {
  key: string
  /**
   * The hint only shows where the OS has the feature behind it (the registry's `needsCapability`), so its
   * denominator also holds installs that could never see it: the popover says so.
   */
  capabilityGated: boolean
}

const flag = (key: string, capabilityGated = false): FunnelFlag => ({ key, capabilityGated })

/** The one step: finished onboarding. */
export const onboardingStep = flag('onboarding.completed')

/** Hints and intros an install has discovered. Rendered sorted by share. */
export const discoveredFlags: readonly FunnelFlag[] = [
  flag('fileExplorer.quickFilterIntroSeen'),
  flag('fileExplorer.git.portalIntroSeen'),
  flag('behavior.doubleClickOnPaneNotificationSeen'),
  flag('network.firstTriggerDone'),
  flag('behavior.serversPinHintSeen'),
  flag('advanced.exitFullScreenOnEscapeHintShown', true),
  flag('behavior.textEditorHintSeen', true),
  flag('behavior.openTerminalHereToastSeen', true),
  flag('behavior.revealActivationNoticeSeen', true),
  flag('behavior.fileSystemWatching.globalGoToLatestShortcut.acknowledged', true),
  flag('indexing.firstStaleDialogShown'),
]

/** Hints that mean someone hit a snag. */
export const frictionFlags: readonly FunnelFlag[] = [flag('fileExplorer.quickFilterTypingHintSeen')]

/** Hints someone dismissed by hand. */
export const dismissedFlags: readonly FunnelFlag[] = [
  flag('behavior.adbHintDismissed'),
  flag('aboutThis.aiHintDismissed'),
]

/** Saw the upgrade nudge: shown on the Money card, not in the funnel. */
export const upgradeNudgeFlag = flag('onboarding.upgradeNudgeShown')

/** Below this many eligible installs a share is noise: print "n of m" and no bar. */
export const minEligible = 5

/** One flag's reach. */
export interface FlagReach extends FunnelFlag {
  eligible: number
  reached: number
  /** `reached ÷ eligible`, or null below `minEligible` eligible installs. */
  share: number | null
}

/** A flag's reach, or null when no install's build has it yet. */
export function flagReach(settings: SettingAdoption[], funnelFlag: FunnelFlag): FlagReach | null {
  const setting = settings.find((s) => s.key === funnelFlag.key)
  if (setting === undefined || setting.eligible === 0) return null
  const reached = setting.eligible - setting.onDefault
  return {
    ...funnelFlag,
    eligible: setting.eligible,
    reached,
    share: setting.eligible >= minEligible ? reached / setting.eligible : null,
  }
}

export interface OnboardingFunnel {
  step: FlagReach | null
  /** Highest share first; flags without a share (too few eligible) last, by key. */
  discovered: FlagReach[]
  friction: FlagReach[]
  dismissed: FlagReach[]
}

function reachAll(settings: SettingAdoption[], flags: readonly FunnelFlag[]): FlagReach[] {
  return flags.flatMap((f) => {
    const reach = flagReach(settings, f)
    return reach === null ? [] : [reach]
  })
}

export function onboardingFunnel(settings: SettingAdoption[]): OnboardingFunnel {
  return {
    step: flagReach(settings, onboardingStep),
    discovered: reachAll(settings, discoveredFlags).sort(
      (a, b) => (b.share ?? -1) - (a.share ?? -1) || a.key.localeCompare(b.key),
    ),
    friction: reachAll(settings, frictionFlags),
    dismissed: reachAll(settings, dismissedFlags),
  }
}
