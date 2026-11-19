import { describe, expect, it, onTestFinished } from 'vitest'
import {
  convention,
  CONVENTIONS_GLOBAL,
  CONVENTIONS_ON,
  FALLBACK_CONVENTIONS,
  pageDrawsMenuBar,
  platformConventions,
  type ConventionsDesktop,
} from './index.js'
import { withConventions } from './test-platform.js'

const host = globalThis as Record<string, unknown>
const desktops = Object.keys(CONVENTIONS_ON) as ConventionsDesktop[]

describe('platformConventions', () => {
  it('reads the fallback platform’s desktop-neutral row where nothing was injected', () => {
    expect(Object.hasOwn(host, CONVENTIONS_GLOBAL)).toBe(false)
    expect(platformConventions()).toEqual(CONVENTIONS_ON.otherLinux)
    expect(FALLBACK_CONVENTIONS).toEqual(CONVENTIONS_ON.otherLinux)
  })

  it.each(desktops)('reads the %s row as the backend injects it', (desktop) => {
    onTestFinished(withConventions(desktop))
    expect(platformConventions()).toEqual(CONVENTIONS_ON[desktop])
  })

  it('falls back field by field on a missing or junk value', () => {
    onTestFinished(() => {
      Reflect.deleteProperty(host, CONVENTIONS_GLOBAL)
    })
    host[CONVENTIONS_GLOBAL] = {
      dialogButtonOrder: 'affirmativeFirst',
      enterActivates: 'sometimes',
      screenReaders: ['nvda', 'clippy'],
      linuxDesktop: 'kde',
    }
    expect(platformConventions()).toEqual({
      ...FALLBACK_CONVENTIONS,
      dialogButtonOrder: 'affirmativeFirst',
      linuxDesktop: 'kde',
    })
    for (const junk of [null, 'windows', 1]) {
      host[CONVENTIONS_GLOBAL] = junk
      expect(platformConventions()).toEqual(FALLBACK_CONVENTIONS)
    }
  })

  it('keeps an injected null desktop: off Linux there is none', () => {
    onTestFinished(withConventions('windows'))
    expect(convention('linuxDesktop')).toBeNull()
  })
})

describe('pageDrawsMenuBar', () => {
  it.each(desktops)('answers for %s: the page draws the bar everywhere but macOS', (desktop) => {
    onTestFinished(withConventions(desktop))
    expect(pageDrawsMenuBar()).toBe(desktop !== 'macos')
  })
})

describe('withConventions', () => {
  it('overrides single fields over a desktop’s row, and undoes it', () => {
    const undo = withConventions('kde', { enterActivates: 'defaultButton' })
    expect(convention('dialogButtonOrder')).toBe('affirmativeFirst')
    expect(convention('enterActivates')).toBe('defaultButton')
    undo()
    expect(Object.hasOwn(host, CONVENTIONS_GLOBAL)).toBe(false)
  })
})

describe('CONVENTIONS_ON', () => {
  // The backend's `conventions_on` pins the rows (a Rust test reads this JSON); these are the
  // splits the UI relies on, stated once in the frontend's words.
  it('splits Linux button order by desktop: GNOME like macOS, KDE like Windows', () => {
    expect(CONVENTIONS_ON.gnome.dialogButtonOrder).toBe(CONVENTIONS_ON.macos.dialogButtonOrder)
    expect(CONVENTIONS_ON.kde.dialogButtonOrder).toBe(CONVENTIONS_ON.windows.dialogButtonOrder)
  })

  it('names a Linux desktop only on Linux', () => {
    expect(CONVENTIONS_ON.macos.linuxDesktop).toBeNull()
    expect(CONVENTIONS_ON.windows.linuxDesktop).toBeNull()
    expect(
      [CONVENTIONS_ON.gnome, CONVENTIONS_ON.kde, CONVENTIONS_ON.otherLinux].map((row) => row.linuxDesktop),
    ).toEqual(['gnome', 'kde', 'other'])
  })
})
