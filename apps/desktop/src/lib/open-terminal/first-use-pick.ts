/**
 * The first-use picker: which terminal the very first "Open terminal here" uses,
 * and whether it teaches the setting afterwards.
 *
 * Neither macOS nor Windows hands Cmdr a default terminal, so Cmdr's own default
 * is the backend's (`defaultId`: Terminal.app on macOS, the first installed of
 * Windows Terminal, PowerShell, and Command Prompt on Windows), and this is the one
 * chance to do better than that without asking anyone anything. Pure, so the whole
 * decision table is testable without probing anything; the caller does the
 * launching, the persisting, and the toast.
 */

import { TERMINAL_APP_BUNDLE_ID } from '#lib/settings/sections/terminal-app-options.js'
import type { TerminalApp } from '#lib/ipc/bindings.js'

/** What the decision reads. */
export interface FirstUsePickInput {
  /** The stored `behavior.openTerminalHereApp` value. */
  storedChoice: string
  /** The stored `behavior.openTerminalHereToastSeen` flag. */
  hintSeen: boolean
  /** The installed terminals, from `list_terminal_apps`. Empty when that query timed out. */
  apps: TerminalApp[]
  /** The app the default launches on this machine (`list_terminal_apps`' `defaultId`). */
  defaultId: string | null
}

/** What the caller then does. */
export interface FirstUsePick {
  /** The app to launch with. */
  appChoice: string
  /** Write `appChoice` back to the setting: the adopted app has to survive not being running. */
  persistChoice: boolean
  /** Raise the one-time hint toast. */
  showHint: boolean
  /** Spend the one-time flag, so the hint never comes back. */
  markSeen: boolean
}

/** Nothing to adopt, nothing to say: launch what's stored and leave the flag alone. */
function quiet(storedChoice: string): FirstUsePick {
  return { appChoice: storedChoice, persistChoice: false, showHint: false, markSeen: false }
}

/**
 * Decide what the first run does.
 *
 * The rules:
 *
 * - The hint is spent, or the list came back empty (a timed-out query knows
 *   nothing): launch what's stored, say nothing.
 * - The default is the only terminal here (Terminal.app alone on a Mac): launch
 *   it, say nothing, and ❗ leave the flag UNSPENT. Someone who installs Ghostty next month is still
 *   owed the hint, and a flag spent today would eat it.
 * - Otherwise the hint is due. If exactly one terminal is running AND the user
 *   hasn't already chosen one, adopt it and write it down: "running right now" is
 *   the only signal available, and next time it may not be.
 */
export function decideFirstUsePick({ storedChoice, hintSeen, apps, defaultId }: FirstUsePickInput): FirstUsePick {
  if (hintSeen || apps.length === 0) return quiet(storedChoice)

  const hasAnotherTerminal = apps.some((app) => app.id !== defaultId)
  if (!hasAnotherTerminal) return quiet(storedChoice)

  const running = apps.filter((app) => app.isRunning)
  // The registry default is "the default" on every OS, whatever it launches.
  const userAlreadyChose = storedChoice !== TERMINAL_APP_BUNDLE_ID
  const adopt = !userAlreadyChose && running.length === 1 ? running[0].id : null

  return {
    appChoice: adopt ?? storedChoice,
    persistChoice: adopt !== null,
    showHint: true,
    markSeen: true,
  }
}
