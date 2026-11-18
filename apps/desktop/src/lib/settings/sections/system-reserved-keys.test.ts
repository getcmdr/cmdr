import { describe, it, expect, onTestFinished } from 'vitest'
import { withConventions } from '#lib/platform/test-platform.js'
import { LINUX_DESKTOPS, reservedCombos, systemReservation } from './system-reserved-keys'

describe('systemReservation', () => {
  it('names a macOS feature by its message key, on macOS only', () => {
    expect(systemReservation('⌘Space', 'macos')).toEqual({ os: 'macos', featureKey: 'shortcuts.system.spotlight' })
    expect(systemReservation('⌘Space', 'linux', null)).toBeNull()
  })

  it('names the Windows feature that takes a combo, on Windows only', () => {
    expect(systemReservation('Alt+Tab', 'windows')).toEqual({
      os: 'windows',
      featureKey: 'shortcuts.system.switchingApps',
    })
    expect(systemReservation('Ctrl+Shift+Escape', 'windows')).toEqual({
      os: 'windows',
      featureKey: 'shortcuts.system.taskManager',
    })
    expect(systemReservation('Ctrl+Shift+Escape', 'macos')).toBeNull()
  })

  it('names EVERY Linux desktop that takes a combo while the running one is unknown, in one fixed order', () => {
    expect(systemReservation('Alt+F2', 'linux', null)).toEqual({ os: 'linux', desktops: [...LINUX_DESKTOPS] })
    expect(systemReservation('Alt+F1', 'linux', null)).toEqual({
      os: 'linux',
      desktops: ['KDE Plasma', 'Xfce', 'Cinnamon'],
    })
    expect(systemReservation('Ctrl+F3', 'linux', null)).toEqual({ os: 'linux', desktops: ['KDE Plasma', 'Xfce'] })
    // Ubuntu is GNOME plus its own keys, so it takes everything GNOME does.
    expect(systemReservation('Alt+F5', 'linux', null)).toEqual({
      os: 'linux',
      desktops: ['GNOME', 'Cinnamon', 'Ubuntu'],
    })
    expect(systemReservation('Ctrl+Alt+D', 'linux', null)).toEqual({ os: 'linux', desktops: ['Xfce', 'Ubuntu'] })
  })

  // Every listed desktop's window switcher and screenshot key, read in each one's own source.
  it('names every desktop for the window manager keys they all bind', () => {
    for (const combo of ['Alt+Tab', 'Alt+Shift+Tab', 'PrintScreen', 'Shift+PrintScreen', 'Alt+F4', 'Alt+Space']) {
      expect(systemReservation(combo, 'linux', null)).toEqual({ os: 'linux', desktops: [...LINUX_DESKTOPS] })
    }
    expect(systemReservation('Alt+`', 'linux', null)).toEqual({
      os: 'linux',
      desktops: ['GNOME', 'KDE Plasma', 'Cinnamon', 'Ubuntu'],
    })
    expect(systemReservation('Ctrl+Alt+L', 'linux', null)).toEqual({ os: 'linux', desktops: ['Xfce', 'Cinnamon'] })
    expect(systemReservation('Ctrl+PrintScreen', 'linux', null)).toEqual({ os: 'linux', desktops: ['Cinnamon'] })
  })

  it('leaves a key no listed desktop takes alone', () => {
    expect(systemReservation('Alt+Shift+P', 'linux', null)).toBeNull()
    expect(systemReservation('Ctrl+Shift+K', 'linux', null)).toBeNull()
    expect(systemReservation('F11', 'linux', null)).toBeNull()
  })

  // The running desktop narrows the list: a GNOME user never hears about a key only KDE or Xfce
  // reserves. GNOME can't be told from Ubuntu's GNOME, so it names both; `other` is Xfce and
  // Cinnamon (and the desktops the table doesn't read).
  it('names only the running desktop once it is known', () => {
    expect(systemReservation('Ctrl+F3', 'linux', 'gnome')).toBeNull()
    expect(systemReservation('Alt+F5', 'linux', 'gnome')).toEqual({ os: 'linux', desktops: ['GNOME', 'Ubuntu'] })
    expect(systemReservation('Alt+F5', 'linux', 'kde')).toBeNull()
    expect(systemReservation('Ctrl+F3', 'linux', 'kde')).toEqual({ os: 'linux', desktops: ['KDE Plasma'] })
    expect(systemReservation('Ctrl+F3', 'linux', 'other')).toEqual({ os: 'linux', desktops: ['Xfce'] })
    expect(systemReservation('Alt+F5', 'linux', 'other')).toEqual({ os: 'linux', desktops: ['Cinnamon'] })
  })

  it('reads the running desktop from the platform conventions by default', () => {
    onTestFinished(withConventions('gnome'))
    expect(systemReservation('Ctrl+F3', 'linux')).toBeNull()
    expect(systemReservation('Alt+F2', 'linux')).toEqual({ os: 'linux', desktops: ['GNOME', 'Ubuntu'] })
  })

  it('lists each platform’s reserved combos for the vocabulary and invariants checks', () => {
    expect(reservedCombos('windows')).toContain('Ctrl+Alt+Delete')
    expect(reservedCombos('linux')).toContain('Ctrl+Alt+→')
    expect(reservedCombos('macos')).toContain('⌘⌥Escape')
  })
})
