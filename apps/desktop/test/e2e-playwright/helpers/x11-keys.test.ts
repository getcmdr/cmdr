import { describe, expect, it } from 'vitest'
import { xdotoolChord } from './x11-keys'

// Regression anchor (review of #1281): the lane's xdotool sends an F-key named by its keysym with
// Alt held, so a "bare F8" was Alt+F8. The helper names every F-key by its keycode instead.
describe('xdotoolChord', () => {
  it('sends every F-key by its keycode, so it arrives bare', () => {
    expect(xdotoolChord('F8')).toBe('74')
    expect(xdotoolChord('F10')).toBe('76')
    expect(xdotoolChord('F1')).toBe('67')
    expect(xdotoolChord('F12')).toBe('96')
  })

  it('keeps the modifiers a chord names beside an F-key', () => {
    expect(xdotoolChord('shift+F8')).toBe('shift+74')
    expect(xdotoolChord('alt+F4')).toBe('alt+70')
  })

  it('leaves every other key as xdotool spells it', () => {
    expect(xdotoolChord('alt+f')).toBe('alt+f')
    expect(xdotoolChord('Escape')).toBe('Escape')
    expect(xdotoolChord('ctrl+Return')).toBe('ctrl+Return')
  })
})
