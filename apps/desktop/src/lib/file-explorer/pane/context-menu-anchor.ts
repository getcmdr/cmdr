/**
 * Where a KEYBOARD-opened file context menu (`⌃⏎`) pops up.
 *
 * The mouse path needs none of this: it passes no position and the OS uses the pointer.
 * A keypress has no pointer, so the pane has to name a point itself.
 *
 * Coordinates are VIEWPORT CSS pixels throughout — what `getBoundingClientRect()`
 * returns, and exactly what the popup IPC takes. `pane/DETAILS.md` § Popping a context
 * menu from the keyboard has the measurement behind that (the webview fills the whole
 * window, so the two spaces coincide) and why it's `Logical`, not physical, on the wire.
 */

/** The part of a `DOMRect` an anchor needs, so callers can measure however they like. */
export interface AnchorRect {
  left: number
  top: number
  bottom: number
}

/** A point in viewport CSS pixels. */
export interface AnchorPoint {
  x: number
  y: number
}

/**
 * How far right of the row's left edge the menu opens, so it doesn't cover the icon
 * the user is looking at. Same inset as `enter-menu.ts`'s popup, deliberately: two
 * menus that open off the cursor row should open in the same place.
 */
const ROW_INSET_PX = 16

/**
 * The anchor point, given whichever rects could be measured.
 *
 * - The cursor row is rendered → just under it, inset from its left edge.
 * - It isn't (the user scrolled away with the scrollbar, or the pane holds 10k rows) →
 *   the scroll surface's left edge, vertically centred. ❌ Never scroll the row back
 *   into view: a keypress that silently moves the user's view is worse than a menu in a
 *   slightly odd spot.
 * - Neither is measurable → `null`, and the caller sends no position at all, leaving the
 *   OS to use the pointer. A degenerate case; a mounted pane always has a surface.
 */
export function contextMenuAnchor(row: AnchorRect | null, surface: AnchorRect | null): AnchorPoint | null {
  if (row) return { x: row.left + ROW_INSET_PX, y: row.bottom }
  if (surface) return { x: surface.left, y: (surface.top + surface.bottom) / 2 }
  return null
}

/**
 * {@link contextMenuAnchor} over a pane's live DOM.
 *
 * ❗ Both panes show a cursor row, so the lookup is scoped to `paneEl`: a document-wide
 * query would hand back the left pane's row whichever pane asked.
 *
 * @param paneEl - The pane's root element (`FilePane`'s `paneEl`).
 * @param measure - How to measure an element; injected so the pure part stays testable
 *                  where `getBoundingClientRect` answers all zeroes.
 */
export function cursorRowAnchor(
  paneEl: HTMLElement | null,
  measure: (el: Element) => AnchorRect = (el) => el.getBoundingClientRect(),
): AnchorPoint | null {
  if (!paneEl) return null
  const surface = paneEl.querySelector('[data-file-list-surface]')
  return rowAnchorIn(paneEl, '.file-entry.is-under-cursor', surface, measure)
}

/**
 * {@link contextMenuAnchor} for the row `rowSelector` finds inside `container`, else
 * `surface`: the same rule for any list that isn't a file list.
 */
export function rowAnchorIn(
  container: HTMLElement | null,
  rowSelector: string,
  surface: Element | null = container,
  measure: (el: Element) => AnchorRect = (el) => el.getBoundingClientRect(),
): AnchorPoint | null {
  const row = container?.querySelector(rowSelector) ?? null
  if (row) return contextMenuAnchor(measure(row), null)
  return contextMenuAnchor(null, surface ? measure(surface) : null)
}
