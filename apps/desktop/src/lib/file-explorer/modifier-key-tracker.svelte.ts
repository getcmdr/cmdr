// Tracks Alt/Option, Cmd, Ctrl, and Shift modifier key state during drag operations.
//
// The drop-operation policy (`drag/drop-operation.ts`) decides what each modifier means, by the
// platform's `dropModifiers` convention (Finder: Alt copies, Cmd or Shift moves; Explorer and
// Linux: Ctrl copies, Shift moves); otherwise the default is volume-aware (same volume → Move,
// cross volume → Copy).
//
// Two sources feed this state:
// 1. Document keydown/keyup events (works when webview is focused)
// 2. The native `drag-modifiers` event: macOS's swizzled WryWebView reads
//    [NSEvent modifierFlags], Windows' drop target OLE's key state. Both work during OS-level
//    drags, when the webview doesn't receive keyboard events.

import type { ModifierState } from './drag/drop-operation'

let altKeyHeld = $state(false)
let cmdKeyHeld = $state(false)
let ctrlKeyHeld = $state(false)
let shiftKeyHeld = $state(false)
let listenerAttached = false

function handleKeyDown(e: KeyboardEvent) {
  if (e.key === 'Alt') altKeyHeld = true
  else if (e.key === 'Meta') cmdKeyHeld = true
  else if (e.key === 'Control') ctrlKeyHeld = true
  else if (e.key === 'Shift') shiftKeyHeld = true
}

function handleKeyUp(e: KeyboardEvent) {
  if (e.key === 'Alt') altKeyHeld = false
  else if (e.key === 'Meta') cmdKeyHeld = false
  else if (e.key === 'Control') ctrlKeyHeld = false
  else if (e.key === 'Shift') shiftKeyHeld = false
}

/** Starts listening for modifier key changes on the document. Idempotent. */
export function startModifierTracking(): void {
  if (listenerAttached) return
  document.addEventListener('keydown', handleKeyDown)
  document.addEventListener('keyup', handleKeyUp)
  listenerAttached = true
}

/** Stops listening and resets state. Idempotent. */
export function stopModifierTracking(): void {
  if (!listenerAttached) return
  document.removeEventListener('keydown', handleKeyDown)
  document.removeEventListener('keyup', handleKeyUp)
  listenerAttached = false
  altKeyHeld = false
  cmdKeyHeld = false
  ctrlKeyHeld = false
  shiftKeyHeld = false
}

/** Returns the current modifier state. */
export function getModifierState(): ModifierState {
  return { altHeld: altKeyHeld, cmdHeld: cmdKeyHeld, ctrlHeld: ctrlKeyHeld, shiftHeld: shiftKeyHeld }
}

/** Sets modifier state from an external source (native drag-modifiers event). */
export function setModifiers(state: ModifierState): void {
  altKeyHeld = state.altHeld
  cmdKeyHeld = state.cmdHeld
  ctrlKeyHeld = state.ctrlHeld
  shiftKeyHeld = state.shiftHeld
}
