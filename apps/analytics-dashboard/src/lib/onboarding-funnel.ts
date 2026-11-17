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
  /** What reaching it means, for the dashboard row ("Saw the quick filter intro"). */
  label: string
  /**
   * The hint only shows where the OS has the feature behind it (the registry's `needsCapability`), so its
   * denominator also holds installs that could never see it: the popover says so.
   */
  capabilityGated: boolean
}

const flag = (key: string, label: string, capabilityGated = false): FunnelFlag => ({ key, label, capabilityGated })

/** The one step: finished onboarding. */
export const onboardingStep = flag('onboarding.completed', 'Finished onboarding')

/** Hints and intros an install has discovered. Rendered sorted by share. */
export const discoveredFlags: readonly FunnelFlag[] = [
  flag('fileExplorer.quickFilterIntroSeen', 'Saw the quick filter intro'),
  flag('fileExplorer.git.portalIntroSeen', 'Opened a git repo’s top folder'),
  flag('behavior.doubleClickOnPaneNotificationSeen', 'Saw the double-click-to-go-up hint'),
  flag('network.firstTriggerDone', 'Started network discovery'),
  flag('behavior.serversPinHintSeen', 'Saw the long Network group hint'),
  flag('advanced.exitFullScreenOnEscapeHintShown', 'Saw the Escape full-screen hint', true),
  flag('behavior.textEditorHintSeen', 'Saw the text editor hint', true),
  flag('behavior.openTerminalHereToastSeen', 'Saw the Open terminal here hint', true),
  flag('behavior.revealActivationNoticeSeen', 'Saw the “Show in Finder” notice', true),
  flag(
    'behavior.fileSystemWatching.globalGoToLatestShortcut.acknowledged',
    'Answered the latest-download shortcut question',
    true,
  ),
  flag('indexing.firstStaleDialogShown', 'Saw the out-of-date index explainer'),
]

/** Hints that mean someone hit a snag. */
export const frictionFlags: readonly FunnelFlag[] = [
  flag('fileExplorer.quickFilterTypingHintSeen', 'Had a key run its command mid-filter'),
]

/** Hints someone dismissed by hand. */
export const dismissedFlags: readonly FunnelFlag[] = [
  flag('behavior.adbHintDismissed', 'Closed the USB debugging hint'),
  flag('aboutThis.aiHintDismissed', 'Closed the AI hint on “Tell me about this”'),
]

/** Saw the upgrade nudge: shown on the Money card, not in the funnel. */
export const upgradeNudgeFlag = flag('onboarding.upgradeNudgeShown', 'Saw the upgrade nudge')

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
