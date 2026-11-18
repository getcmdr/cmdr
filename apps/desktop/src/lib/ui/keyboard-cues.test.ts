import { afterEach, describe, expect, it } from 'vitest'
import { withConventions } from '#lib/platform/test-platform.js'
import { createKeyboardCues, cueRule, lastInputWasKeyboard } from './keyboard-cues'

function keydown(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })
}

const ALT = { key: 'Alt', code: 'AltLeft', altKey: true }

afterEach(() => {
  document.documentElement.classList.remove('underline-access-keys')
})

describe('cueRule', () => {
  it('follows the access-keys convention, never the platform', () => {
    withConventions('windows')
    expect(cueRule()).toBe('afterKeyboardUse')
    withConventions('kde')
    expect(cueRule()).toBe('whileAltHeld')
    withConventions('macos')
    expect(cueRule()).toBe('none')
  })
})

describe('lastInputWasKeyboard', () => {
  it('remembers whether a key or a pointer came last', () => {
    window.dispatchEvent(keydown({ key: 'F2' }))
    expect(lastInputWasKeyboard()).toBe(true)
    window.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    expect(lastInputWasKeyboard()).toBe(false)
  })
})

describe('the Win32 keyboard-cue rule (afterKeyboardUse)', () => {
  it('shows from the start in a surface the keyboard opened', () => {
    const cues = createKeyboardCues({ rule: 'afterKeyboardUse', openedByKeyboard: true })
    expect(cues.shown).toBe(true)
    cues.destroy()
  })

  it('hides in a surface a click opened, until a key is used, then stays', () => {
    const cues = createKeyboardCues({ rule: 'afterKeyboardUse', openedByKeyboard: false })
    expect(cues.shown).toBe(false)
    cues.keydown(keydown({ key: 'ArrowDown' }))
    expect(cues.shown).toBe(true)
    cues.keyup(new KeyboardEvent('keyup', { key: 'ArrowDown' }))
    cues.pointerdown()
    expect(cues.shown).toBe(true)
    cues.destroy()
  })

  it('counts Alt, Tab, arrows, F10, and Alt chords, never typing', () => {
    for (const init of [ALT, { key: 'Tab' }, { key: 'ArrowLeft' }, { key: 'F10' }, { key: 's', altKey: true }]) {
      const cues = createKeyboardCues({ rule: 'afterKeyboardUse', openedByKeyboard: false })
      cues.keydown(keydown(init))
      expect(cues.shown, JSON.stringify(init)).toBe(true)
      cues.destroy()
    }
    const typing = createKeyboardCues({ rule: 'afterKeyboardUse', openedByKeyboard: false })
    typing.keydown(keydown({ key: 'a' }))
    typing.keydown(keydown({ key: 'Shift' }))
    expect(typing.shown).toBe(false)
    typing.destroy()
  })

  it('ignores AltGr, which types characters', () => {
    const cues = createKeyboardCues({ rule: 'afterKeyboardUse', openedByKeyboard: false })
    cues.keydown(keydown({ key: 'AltGraph', code: 'AltRight', altKey: true, ctrlKey: true }))
    cues.keydown(keydown({ key: 'e', altKey: true, ctrlKey: true }))
    expect(cues.shown).toBe(false)
    cues.destroy()
  })

  it('reveal() shows them, for a surface whose own keys decide what counts', () => {
    const cues = createKeyboardCues({ rule: 'afterKeyboardUse', openedByKeyboard: false })
    cues.reveal()
    expect(cues.shown).toBe(true)
    cues.destroy()
  })
})

describe('the hold-Alt rule (whileAltHeld)', () => {
  it('shows while Alt is held, and hides on release, another key, a click, or blur', () => {
    const cues = createKeyboardCues({ rule: 'whileAltHeld', openedByKeyboard: true })
    expect(cues.shown).toBe(false)
    cues.keydown(keydown(ALT))
    expect(cues.shown).toBe(true)
    cues.keyup(new KeyboardEvent('keyup', { key: 'Alt' }))
    expect(cues.shown).toBe(false)
    cues.keydown(keydown(ALT))
    cues.keydown(keydown({ key: 's', altKey: true }))
    expect(cues.shown).toBe(false)
    cues.keydown(keydown(ALT))
    cues.pointerdown()
    expect(cues.shown).toBe(false)
    cues.keydown(keydown(ALT))
    cues.blur()
    expect(cues.shown).toBe(false)
    cues.destroy()
  })
})

describe("Windows' Underline access keys setting", () => {
  it('shows them always, under either rule, and follows the class live', async () => {
    document.documentElement.classList.add('underline-access-keys')
    const win = createKeyboardCues({ rule: 'afterKeyboardUse', openedByKeyboard: false })
    const linux = createKeyboardCues({ rule: 'whileAltHeld', openedByKeyboard: false })
    expect(win.shown).toBe(true)
    expect(linux.shown).toBe(true)
    const changes: boolean[] = []
    const off = win.onChange((shown) => changes.push(shown))
    document.documentElement.classList.remove('underline-access-keys')
    await Promise.resolve()
    expect(win.shown).toBe(false)
    expect(changes).toEqual([false])
    off()
    win.destroy()
    linux.destroy()
  })

  it('never shows them where the platform has no access keys', () => {
    document.documentElement.classList.add('underline-access-keys')
    const cues = createKeyboardCues({ rule: 'none', openedByKeyboard: true })
    cues.keydown(keydown(ALT))
    expect(cues.shown).toBe(false)
    cues.destroy()
  })
})
