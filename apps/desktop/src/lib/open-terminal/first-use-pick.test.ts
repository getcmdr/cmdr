import { describe, it, expect } from 'vitest'
import { decideFirstUsePick } from './first-use-pick'
import { TERMINAL_APP_BUNDLE_ID } from '#lib/settings/sections/terminal-app-options.js'
import type { TerminalApp } from '#lib/ipc/bindings.js'

function app(id: string, isRunning = false): TerminalApp {
  return { id, displayName: id, icon: null, isRunning }
}

const WARP = 'dev.warp.Warp-Stable'
const GHOSTTY = 'com.mitchellh.ghostty'

describe('decideFirstUsePick', () => {
  it('changes nothing once the hint has been shown', () => {
    const pick = decideFirstUsePick({
      storedChoice: TERMINAL_APP_BUNDLE_ID,
      hintSeen: true,
      defaultId: TERMINAL_APP_BUNDLE_ID,
      apps: [app(TERMINAL_APP_BUNDLE_ID), app(WARP, true)],
    })
    expect(pick).toEqual({ appChoice: TERMINAL_APP_BUNDLE_ID, persistChoice: false, showHint: false, markSeen: false })
  })

  it('stays quiet when Terminal is the only terminal installed, and keeps the hint unspent', () => {
    // A user who installs Warp next month is still owed the hint, so the flag
    // must NOT be marked seen here.
    const pick = decideFirstUsePick({
      storedChoice: TERMINAL_APP_BUNDLE_ID,
      hintSeen: false,
      defaultId: TERMINAL_APP_BUNDLE_ID,
      apps: [app(TERMINAL_APP_BUNDLE_ID, true)],
    })
    expect(pick).toEqual({ appChoice: TERMINAL_APP_BUNDLE_ID, persistChoice: false, showHint: false, markSeen: false })
  })

  it('adopts the one running terminal and remembers it', () => {
    // "Running right now" is the only signal available on the first run, and it's
    // persisted because next time it may not be running.
    const pick = decideFirstUsePick({
      storedChoice: TERMINAL_APP_BUNDLE_ID,
      hintSeen: false,
      defaultId: TERMINAL_APP_BUNDLE_ID,
      apps: [app(TERMINAL_APP_BUNDLE_ID), app(WARP, true), app(GHOSTTY)],
    })
    expect(pick).toEqual({ appChoice: WARP, persistChoice: true, showHint: true, markSeen: true })
  })

  it('keeps the stored choice when two terminals are running, and still shows the hint', () => {
    const pick = decideFirstUsePick({
      storedChoice: TERMINAL_APP_BUNDLE_ID,
      hintSeen: false,
      defaultId: TERMINAL_APP_BUNDLE_ID,
      apps: [app(TERMINAL_APP_BUNDLE_ID, true), app(WARP, true), app(GHOSTTY)],
    })
    expect(pick).toEqual({ appChoice: TERMINAL_APP_BUNDLE_ID, persistChoice: false, showHint: true, markSeen: true })
  })

  it('keeps the stored choice when none is running, and still shows the hint', () => {
    const pick = decideFirstUsePick({
      storedChoice: TERMINAL_APP_BUNDLE_ID,
      hintSeen: false,
      defaultId: TERMINAL_APP_BUNDLE_ID,
      apps: [app(TERMINAL_APP_BUNDLE_ID), app(WARP)],
    })
    expect(pick).toEqual({ appChoice: TERMINAL_APP_BUNDLE_ID, persistChoice: false, showHint: true, markSeen: true })
  })

  it('never overwrites a choice the user already made', () => {
    // Ghostty is running, but the user picked Warp in Settings. Adopting Ghostty
    // would undo that in silence.
    const pick = decideFirstUsePick({
      storedChoice: WARP,
      hintSeen: false,
      defaultId: TERMINAL_APP_BUNDLE_ID,
      apps: [app(TERMINAL_APP_BUNDLE_ID), app(WARP), app(GHOSTTY, true)],
    })
    expect(pick).toEqual({ appChoice: WARP, persistChoice: false, showHint: true, markSeen: true })
  })

  it('falls back to the stored choice when the app list came back empty', () => {
    // The list query timed out. Nothing is known, so nothing is claimed: launch
    // what's stored and leave the hint for a run that can answer.
    const pick = decideFirstUsePick({
      storedChoice: TERMINAL_APP_BUNDLE_ID,
      hintSeen: false,
      defaultId: null,
      apps: [],
    })
    expect(pick).toEqual({ appChoice: TERMINAL_APP_BUNDLE_ID, persistChoice: false, showHint: false, markSeen: false })
  })

  describe('on Windows, where the default is whatever the backend finds first', () => {
    const WT = 'windows-terminal'
    const CMD = 'cmd'

    it('stays quiet when the default is the only terminal, even though the stored value is the registry default', () => {
      const pick = decideFirstUsePick({
        storedChoice: TERMINAL_APP_BUNDLE_ID,
        hintSeen: false,
        defaultId: CMD,
        apps: [app(CMD)],
      })
      expect(pick).toEqual({
        appChoice: TERMINAL_APP_BUNDLE_ID,
        persistChoice: false,
        showHint: false,
        markSeen: false,
      })
    })

    it('shows the hint when there is a terminal besides the default, and keeps launching the default', () => {
      const pick = decideFirstUsePick({
        storedChoice: TERMINAL_APP_BUNDLE_ID,
        hintSeen: false,
        defaultId: WT,
        apps: [app(WT), app(CMD)],
      })
      expect(pick).toEqual({ appChoice: TERMINAL_APP_BUNDLE_ID, persistChoice: false, showHint: true, markSeen: true })
    })
  })
})
