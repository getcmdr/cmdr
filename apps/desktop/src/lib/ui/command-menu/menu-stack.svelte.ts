/**
 * An open command menu and its submenus, as state: which surfaces are up, the rows each shows,
 * and where each one's cursor is. The one stack the menu bar's flyouts and the context menus
 * share (`MenuSurfaces.svelte` draws it). It walks, opens, and picks; what a pick SENDS is the
 * caller's (`onPick`), and which keys reach it is the caller's too (`menu-bar.svelte.ts`,
 * `context-menu.svelte.ts`). A greyed row never picks.
 */

import type { Row } from '#lib/ipc/bindings.js'
import type { PlacementRect } from '../anchored-placement'
import { edgeLive, isLive, nextLive, rowAccessKeys, type MenuFlavor, type PickableRow } from './rows'

/** Where a stack's first surface hangs: under a bar title, or at a point (a context menu). */
export type RootAnchor = { kind: 'below'; rect: PlacementRect } | { kind: 'point'; x: number; y: number }

/** Who moved a surface's cursor last, which decides how the row shows it (a fill, or the focus rings). */
export type HighlightSource = 'keyboard' | 'pointer'

export interface StackSurface {
  /** Unique per open, for `id` and `aria-controls`. */
  readonly id: string
  readonly rows: Row[]
  /** Each row's access key, or null. Empty where the flavor has none. */
  readonly letters: (string | null)[]
  /** The cursor's row index, `-1` for none. */
  highlighted: number
  highlightBy: HighlightSource
  /** The row in the surface below that this submenu hangs off; null for the first surface. */
  readonly parentIndex: number | null
  /** Where the first surface hangs; a submenu hangs beside its parent row. */
  readonly anchor: RootAnchor | null
  /** The accessible name, when no element names the surface. */
  readonly label?: string
  /** The element that names the surface (a bar title). */
  readonly labelledBy?: string
}

export interface OpenOptions {
  /** Opened from the keyboard: the cursor starts on the first live row (the last with `fromEnd`). */
  fromKeyboard: boolean
  fromEnd?: boolean
  label?: string
  labelledBy?: string
  /**
   * The element that keeps DOM focus while the menu is up (a text field's edit menu): the rows
   * never take it, the cursor reaches a screen reader as its `aria-activedescendant`, so the
   * field never blurs.
   */
  focusHolder?: HTMLElement
}

export interface MenuStackDeps {
  flavor: () => MenuFlavor
  /** Whether rows carry access keys (the `desktop` flavor, on a platform that has them). */
  accessKeys: () => boolean
  /** A live command or check was picked. The stack stays as it is; the caller closes it. */
  onPick: (row: PickableRow) => void
}

/**
 * How long a hovered submenu row waits before its submenu opens, or a hovered sibling before the
 * open one closes: Windows' default `MenuShowDelay` (400 ms, `HKCU\Control Panel\Desktop`), so a
 * diagonal move toward a submenu doesn't close it on the way. The macOS flavor opens at once, as
 * the demo's bar always has.
 */
export const DESKTOP_SUBMENU_DELAY_MS = 400

export interface MenuStack {
  readonly surfaces: readonly StackSurface[]
  readonly isOpen: boolean
  /** `OpenOptions.focusHolder` of the open menu, or null where the rows hold focus themselves. */
  readonly focusHolder: HTMLElement | null
  /**
   * The surface the keyboard is in: the deepest one with a cursor. A submenu the pointer only
   * hovered open has none, so the keys still walk its parent (and close it), as on Windows.
   */
  readonly activeDepth: number
  /** Opens `rows` as the first surface, closing whatever was open. */
  open(rows: Row[], anchor: RootAnchor, options: OpenOptions): StackSurface
  close(): void
  /** ↑ / ↓ on the innermost surface. */
  walk(step: 1 | -1): void
  /** Home / End. */
  edge(first: boolean): void
  /** → on a live submenu row: opens it with the cursor inside. False when the cursor isn't on one. */
  enterSubmenu(): boolean
  /** ← and Esc inside a submenu: closes it, the cursor back on its row. False at the first surface. */
  backOut(): boolean
  /** Enter / Space: picks the cursor's row, or enters its submenu. False with no cursor. */
  pickHighlighted(): boolean
  /** A letter typed in the menu: the innermost surface's row with that access key acts. */
  typeAhead(letter: string): boolean
  /** The pointer reached a row. */
  hoverRow(depth: number, index: number): void
  /** A click on a row. */
  clickRow(depth: number, index: number): void
  /** The first surface's rows moved (a live check, a label): redrawn in place where the shape held, else closed. */
  refresh(rows: Row[]): void
  destroy(): void
}

let nextSurfaceId = 0

export function createMenuStack(deps: MenuStackDeps): MenuStack {
  let surfaces = $state<StackSurface[]>([])
  let focusHolder = $state<HTMLElement | null>(null)
  let pending: ReturnType<typeof setTimeout> | null = null

  const delay = (): number => (deps.flavor() === 'desktop' ? DESKTOP_SUBMENU_DELAY_MS : 0)

  function cancelPending(): void {
    if (pending !== null) clearTimeout(pending)
    pending = null
  }

  /** Runs `action` after the submenu delay, or now where there is none. A newer hover replaces it. */
  function later(action: () => void): void {
    cancelPending()
    const ms = delay()
    if (ms === 0) {
      action()
      return
    }
    pending = setTimeout(() => {
      pending = null
      action()
    }, ms)
  }

  function build(
    rows: Row[],
    parentIndex: number | null,
    anchor: RootAnchor | null,
    naming: Partial<OpenOptions>,
  ): StackSurface {
    return {
      id: `command-menu-${String(nextSurfaceId++)}`,
      rows,
      letters: deps.accessKeys() ? rowAccessKeys(rows) : [],
      highlighted: -1,
      highlightBy: 'pointer',
      parentIndex,
      anchor,
      label: naming.label,
      labelledBy: naming.labelledBy,
    }
  }

  function closeFrom(depth: number): void {
    if (depth < surfaces.length) surfaces = surfaces.slice(0, depth)
  }

  /** `MenuStack.activeDepth`. */
  function activeDepth(): number {
    let depth = surfaces.length - 1
    while (depth > 0 && surfaces[depth].highlighted < 0) depth--
    return depth
  }

  function setHighlight(surface: StackSurface, index: number, by: HighlightSource): void {
    surface.highlighted = index
    surface.highlightBy = by
  }

  /** Opens the submenu of the row at `index` on the surface at `depth`, its cursor inside when `withCursor`. */
  function openSubmenu(depth: number, index: number, withCursor: boolean): void {
    const parent = surfaces.at(depth)
    const row = parent?.rows.at(index)
    if (!parent || row?.kind !== 'submenu' || !row.enabled) return
    const open = surfaces.at(depth + 1)
    if (open?.parentIndex === index) {
      if (withCursor) setHighlight(open, edgeLive(open.rows, true), 'keyboard')
      return
    }
    closeFrom(depth + 1)
    const child = build(row.rows, index, null, {})
    if (withCursor) setHighlight(child, edgeLive(child.rows, true), 'keyboard')
    surfaces = [...surfaces, child]
  }

  /** Acts on a row: a submenu opens (its cursor inside), a command or check picks. */
  function activate(depth: number, index: number, withCursor: boolean): boolean {
    const row = surfaces.at(depth)?.rows.at(index)
    if (!isLive(row)) return false
    if (row.kind === 'submenu') {
      openSubmenu(depth, index, withCursor)
      return true
    }
    cancelPending()
    deps.onPick(row)
    return true
  }

  return {
    get surfaces() {
      return surfaces
    },
    get isOpen() {
      return surfaces.length > 0
    },
    get focusHolder() {
      return focusHolder
    },
    get activeDepth() {
      return activeDepth()
    },
    open(rows, anchor, options) {
      cancelPending()
      const surface = build(rows, null, anchor, options)
      if (options.fromKeyboard) setHighlight(surface, edgeLive(rows, !options.fromEnd), 'keyboard')
      focusHolder = options.focusHolder ?? null
      surfaces = [surface]
      // The proxy, so later writes to its cursor are seen.
      return surfaces[0]
    },
    close() {
      cancelPending()
      surfaces = []
      focusHolder = null
    },
    walk(step) {
      const depth = activeDepth()
      const surface = surfaces.at(depth)
      if (!surface) return
      cancelPending()
      closeFrom(depth + 1)
      setHighlight(surface, nextLive(surface.rows, surface.highlighted, step), 'keyboard')
    },
    edge(first) {
      const depth = activeDepth()
      const surface = surfaces.at(depth)
      if (!surface) return
      cancelPending()
      closeFrom(depth + 1)
      setHighlight(surface, edgeLive(surface.rows, first), 'keyboard')
    },
    enterSubmenu() {
      const depth = activeDepth()
      const surface = surfaces.at(depth)
      const row = surface?.rows.at(surface.highlighted)
      if (!surface || row?.kind !== 'submenu' || !row.enabled) return false
      cancelPending()
      openSubmenu(depth, surface.highlighted, true)
      return true
    },
    backOut() {
      if (surfaces.length < 2) return false
      cancelPending()
      closeFrom(surfaces.length - 1)
      const parent = surfaces.at(-1)
      if (parent) parent.highlightBy = 'keyboard'
      return true
    },
    pickHighlighted() {
      const depth = activeDepth()
      const surface = surfaces.at(depth)
      if (!surface || surface.highlighted < 0) return false
      activate(depth, surface.highlighted, true)
      return true
    },
    typeAhead(letter) {
      const depth = activeDepth()
      const surface = surfaces.at(depth)
      if (!surface) return false
      const index = surface.letters.indexOf(letter)
      if (index < 0 || !isLive(surface.rows[index])) return false
      setHighlight(surface, index, 'keyboard')
      return activate(depth, index, true)
    },
    hoverRow(depth, index) {
      const surface = surfaces.at(depth)
      if (!surface) return
      const row = surface.rows.at(index)
      // The pointer reached this surface: whatever was about to close it is called off.
      if (!isLive(row)) {
        cancelPending()
        setHighlight(surface, -1, 'pointer')
        later(() => {
          closeFrom(depth + 1)
        })
        return
      }
      setHighlight(surface, index, 'pointer')
      if (surfaces.at(depth + 1)?.parentIndex === index) {
        cancelPending()
        return
      }
      later(() => {
        if (row.kind === 'submenu') openSubmenu(depth, index, false)
        else closeFrom(depth + 1)
      })
    },
    clickRow(depth, index) {
      activate(depth, index, true)
    },
    refresh(rows) {
      const first = surfaces.at(0)
      if (!first) return
      // Same rows, moved state: redraw in place, cursor and submenus kept. A shape change closes.
      const sameShape =
        rows.length === first.rows.length && rows.every((row, index) => row.kind === first.rows[index].kind)
      if (!sameShape) {
        cancelPending()
        surfaces = []
        return
      }
      const next: StackSurface[] = []
      let source: Row[] = rows
      for (const surface of surfaces) {
        next.push({ ...surface, rows: source })
        const child = surfaces.at(next.length)
        const parentRow = child?.parentIndex == null ? undefined : source.at(child.parentIndex)
        if (!child) break
        if (parentRow?.kind !== 'submenu') break
        source = parentRow.rows
      }
      surfaces = next
    },
    destroy() {
      cancelPending()
      surfaces = []
    },
  }
}
