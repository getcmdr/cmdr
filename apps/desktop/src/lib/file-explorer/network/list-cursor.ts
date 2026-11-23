/**
 * Where an arrow key takes a network list's cursor, over `total` rows: up and down
 * one row, left and right to the first and last, like the file list's Brief view.
 * `null` for any other key. Shared by the servers hub (its "Add server…" row
 * counted in `total`) and a host's places, so the two lists move alike.
 */
export function cursorAfterArrow(key: string, cursor: number, total: number): number | null {
  switch (key) {
    case 'ArrowDown':
      return Math.min(cursor + 1, total - 1)
    case 'ArrowUp':
      return Math.max(cursor - 1, 0)
    case 'ArrowLeft':
      return 0
    case 'ArrowRight':
      return total - 1
    default:
      return null
  }
}

/**
 * Whether a network list's row name starts with what the person typed: case-insensitive, and
 * blind to Unicode form, since a server can spell its name decomposed while a keyboard types it
 * composed. Both lists' type-to-jump asks this. Unlike a folder's fuzzy backend match, these
 * lists are short and in memory, so a plain prefix is the whole rule.
 */
export function nameStartsWith(name: string, prefix: string): boolean {
  return name.normalize('NFC').toLowerCase().startsWith(prefix.normalize('NFC').toLowerCase())
}

/**
 * Where the row named `name` sits, or -1: case-insensitive and blind to Unicode form, since
 * a kernel mount can spell a share decomposed while its server's share list spells it
 * composed. The cursor landing back on the share `..` came from asks this (#1238).
 */
export function indexOfName(rows: readonly { name: string }[], name: string): number {
  const wanted = name.normalize('NFC').toLowerCase()
  return rows.findIndex((row) => row.name.normalize('NFC').toLowerCase() === wanted)
}

/** Scrolls a network list just enough to show the row at `index`. Both lists have fixed-height rows. */
export function scrollRowIntoView(
  container: HTMLElement,
  index: number,
  rowHeight: number,
  viewportHeight: number,
): void {
  const targetTop = index * rowHeight
  const targetBottom = targetTop + rowHeight
  if (targetTop < container.scrollTop) {
    container.scrollTop = targetTop
  } else if (targetBottom > container.scrollTop + viewportHeight) {
    container.scrollTop = targetBottom - viewportHeight
  }
}
