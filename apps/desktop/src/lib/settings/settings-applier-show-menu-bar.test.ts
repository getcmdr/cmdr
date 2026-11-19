/**
 * The settings-applier's wiring for `appearance.menuBar`: View > Show menu bar's check (only a
 * page draws it, off macOS) follows the setting whoever writes it, resolved against the desktop
 * (`desktopDefault` is the bar everywhere but GNOME), and starts out that way too, since the
 * backend starts it checked.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { withCapabilities, withConventions } from '#lib/platform/test-platform.js'

const syncMenuShowMenuBar = vi.fn<(checked: boolean) => Promise<void>>(() => Promise.resolve())

/** The change listener the applier registers, captured so the test can fire it. */
let changeListener: ((change: { id: string; value: unknown }) => void) | undefined

vi.mock('#lib/tauri-commands/index.js', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const noop = () => Promise.resolve()
  const stubbed: Record<string, unknown> = {}
  for (const key of Object.keys(actual)) {
    stubbed[key] = typeof actual[key] === 'function' ? noop : actual[key]
  }
  stubbed.syncMenuShowMenuBar = (checked: boolean) => syncMenuShowMenuBar(checked)
  return stubbed
})

vi.mock('#lib/settings/index.js', async (importOriginal) => {
  const actual = await importOriginal<typeof import('#lib/settings/index.js')>()
  return {
    ...actual,
    initializeSettings: vi.fn().mockResolvedValue(undefined),
    onSettingChange: (listener: (change: { id: string; value: unknown }) => void) => {
      changeListener = listener
      return () => {
        changeListener = undefined
      }
    },
  }
})

import { initSettingsApplier, cleanupSettingsApplier } from './settings-applier'

const undos: (() => void)[] = []

beforeEach(() => {
  syncMenuShowMenuBar.mockClear()
  changeListener = undefined
})

afterEach(() => {
  cleanupSettingsApplier()
  for (const undo of undos.splice(0)) undo()
})

describe('settings-applier: appearance.menuBar', () => {
  it('pushes the desktop default at startup: off on GNOME, on on KDE', async () => {
    undos.push(withCapabilities('hideableMenuBar'), withConventions('gnome'))
    await initSettingsApplier()
    expect(syncMenuShowMenuBar).toHaveBeenCalledExactlyOnceWith(false)
    cleanupSettingsApplier()
    syncMenuShowMenuBar.mockClear()
    undos.push(withConventions('kde'))
    await initSettingsApplier()
    expect(syncMenuShowMenuBar).toHaveBeenCalledExactlyOnceWith(true)
  })

  it('pushes nothing at startup where the bar is the system’s (macOS)', async () => {
    undos.push(withCapabilities())
    await initSettingsApplier()
    expect(syncMenuShowMenuBar).not.toHaveBeenCalled()
  })

  it('pushes every change, whoever made it', async () => {
    undos.push(withCapabilities('hideableMenuBar'), withConventions('gnome'))
    await initSettingsApplier()
    syncMenuShowMenuBar.mockClear()
    changeListener?.({ id: 'appearance.menuBar', value: 'shown' })
    expect(syncMenuShowMenuBar).toHaveBeenLastCalledWith(true)
    changeListener?.({ id: 'appearance.menuBar', value: 'hidden' })
    expect(syncMenuShowMenuBar).toHaveBeenLastCalledWith(false)
  })
})
