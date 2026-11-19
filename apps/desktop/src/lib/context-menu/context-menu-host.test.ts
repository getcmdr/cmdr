/**
 * `isKeyboardContextMenu`: Chromium (WebView2) fires `contextmenu` as a `PointerEvent`, whose
 * `pointerType` is empty for a key, so that branch is the live one there; a plain `MouseEvent`
 * falls back to the pressed button.
 */

import { afterEach, describe, expect, it } from 'vitest'
import { isKeyboardContextMenu } from './context-menu-host.svelte'

const original = globalThis.PointerEvent

afterEach(() => {
  globalThis.PointerEvent = original
})

/** jsdom may lack `PointerEvent`; Chromium's carries `pointerType`, which is all this reads. */
function pointerEventClass(): typeof PointerEvent {
  if (typeof original !== 'undefined') return original
  class TestPointerEvent extends MouseEvent {
    readonly pointerType: string
    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init)
      this.pointerType = init.pointerType ?? ''
    }
  }
  globalThis.PointerEvent = TestPointerEvent as unknown as typeof PointerEvent
  return globalThis.PointerEvent
}

describe('isKeyboardContextMenu', () => {
  it("reads a Chromium contextmenu's pointer type: empty for a key, the device for a press", () => {
    const Pointer = pointerEventClass()
    expect(isKeyboardContextMenu(new Pointer('contextmenu', { pointerType: '', button: 0 }))).toBe(true)
    expect(isKeyboardContextMenu(new Pointer('contextmenu', { pointerType: 'mouse', button: 2 }))).toBe(false)
    expect(isKeyboardContextMenu(new Pointer('contextmenu', { pointerType: 'touch', button: 0 }))).toBe(false)
    expect(isKeyboardContextMenu(new Pointer('contextmenu', { pointerType: 'pen', button: 0 }))).toBe(false)
  })

  it('falls back to the pressed button for an engine that fires a plain MouseEvent', () => {
    expect(isKeyboardContextMenu(new MouseEvent('contextmenu', { button: 0 }))).toBe(true)
    expect(isKeyboardContextMenu(new MouseEvent('contextmenu', { button: 2 }))).toBe(false)
  })
})
