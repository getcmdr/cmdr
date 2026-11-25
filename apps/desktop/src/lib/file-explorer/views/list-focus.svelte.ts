/**
 * DOM focus and `aria-activedescendant` for a file list (`BriefList`, `FullList`).
 *
 * The focused pane's listbox holds DOM focus and names its cursor row through
 * `aria-activedescendant`; focus never sits on a row (rows unmount under virtualization, and
 * focus on a removed node drops to `<body>`). The id lands one frame after the listbox takes
 * focus, and again one frame after the cursor row's node is replaced under it, both through
 * `#lib/ui/deferred-active-descendant.svelte.ts`, where the VoiceOver evidence lives.
 *
 * The listbox reports focus from its own `focus` / `blur` events, so every path in counts:
 * `focusList`, a click (the browser's mousedown default), the explorer's focus guard. It
 * also re-homes focus when it mounts or unmounts (below), through `#lib/ui/focus-restore.ts`.
 */

import { untrack } from 'svelte'
import { createDeferredActiveDescendant, focusLaidOut } from '#lib/ui/deferred-active-descendant.svelte.js'
import { focusHome, isFocusAdrift } from '#lib/ui/focus-restore.js'

/** The rendered cursor row: its document-unique id, and the key of the node behind it (its path). */
export interface CursorRowRef {
  id: string
  key: string
}

export interface ListFocus {
  /** The listbox's `aria-activedescendant`: the rendered cursor row, while the listbox has focus. */
  activeDescendant: () => string | undefined
  /** Focuses the listbox without scrolling (it's taller than its scroller); false when it isn't mounted. */
  focus: () => boolean
  /** Bind to the listbox's `onfocus`. */
  handleFocus: () => void
  /** Bind to the listbox's `onblur`. */
  handleBlur: () => void
}

export interface ListFocusDeps {
  /** The rendered cursor row, `undefined` when the cursor names nothing in the virtual window. */
  getCursorRow: () => CursorRowRef | undefined
  /** The listbox element, once mounted. */
  getListbox: () => HTMLElement | undefined
  /** Whether this list's pane is the focused one. */
  isFocusedPane: () => boolean
  /** The cursor row just landed as the active descendant (after focus, or a navigation). */
  onLanded?: () => void
}

/** Call during component init (it installs effects). */
export function createListFocus(deps: ListFocusDeps): ListFocus {
  const { getCursorRow } = deps
  const descendant = createDeferredActiveDescendant({ onLand: () => deps.onLanded?.() })
  let last: CursorRowRef | undefined

  // ❗ Re-homing. A list that remounts (Brief/Full toggle, search results opening or closing)
  // takes its focused node with it, focus drops to `<body>`, and no pane key arrives (2026-10-15
  // spike: ↓ did nothing until `Tab`). So a list mounting in the focused pane picks focus
  // up, and one unmounting puts it home (the next list, or the explorer container when the
  // pane now shows none), both only while focus is adrift (`isFocusAdrift`).
  $effect(() => {
    const listbox = deps.getListbox()
    // The focus is untracked: the focus handlers it runs must not hand their `$state` reads to
    // this effect, which would then re-run on them (#1464, `pane/DETAILS.md` § Focus model).
    if (listbox && untrack(deps.isFocusedPane) && isFocusAdrift()) untrack(() => focusLaidOut(listbox))
  })
  $effect(() => () => {
    // After the flush, so the replacement list (if any) is mounted and bound.
    queueMicrotask(() => {
      if (isFocusAdrift()) focusHome()
    })
  })

  // ❗ A navigation re-renders the cursor row as a NEW node under the same id (row 0 in
  // both folders), and WebKit keeps its focus on the dead one: VoiceOver stays silent on
  // entering a folder. So when the row appears, or its node changes under an unchanged id,
  // the id lands again a frame later. A plain cursor move changes the id and shows at once.
  //
  // ❗ A navigation can also strand focus without remounting the list: Backspace on a focused
  // path segment goes up and unmounts that segment (it turns into the current folder's plain
  // text), so focus falls to `<body>`. The new folder's cursor row picks it up (its own
  // `focus` event lands the id).
  $effect(() => {
    const row = getCursorRow()
    untrack(() => {
      const listbox = deps.getListbox()
      if (row && listbox && last?.key !== row.key && deps.isFocusedPane() && isFocusAdrift()) {
        focusLaidOut(listbox)
      } else if (row && (!last || (last.id === row.id && last.key !== row.key))) {
        descendant.renew(row.id)
      }
      last = row
    })
  })

  return {
    activeDescendant: () => descendant.of(getCursorRow()?.id),
    focus() {
      const listbox = deps.getListbox()
      if (!listbox?.isConnected) return false
      focusLaidOut(listbox)
      return true
    },
    handleFocus() {
      descendant.tookFocus(getCursorRow()?.id)
    },
    handleBlur() {
      descendant.reset()
    },
  }
}
