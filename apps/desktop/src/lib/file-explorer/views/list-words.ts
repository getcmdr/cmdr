/**
 * The words a pane's KIND gives its list where a folder's defaults don't fit: what the
 * Ext column is headed, what the `..` row says, and what an empty list says. The pane
 * fills it from its kind (`../pane/list-kind-words.ts`); both views and the footer take
 * it as a plain prop, so ❌ no view ever asks which kind it shows. Every field left out
 * keeps the folder's own words.
 */
export interface ListWords {
  /** The Ext column's header. */
  extHeader?: string
  /**
   * The Ext column carries prose (a share's description) rather than an extension, so Full
   * measures it up to a sentence's width (`PROSE_CAP_SAMPLE`) instead of an extension's.
   */
  extIsProse?: boolean
  /** No row carries a date, so Full shows no date column (`fitFullListColumns`'s `hasDates`). */
  rowsHaveNoDates?: boolean
  /**
   * No row carries a size (a share list: the network volume is unindexed and a share row's
   * path is no filesystem path, so it could only ever say `<dir>`), so Full shows no Size
   * column (`fitFullListColumns`'s `hasSizes`).
   */
  rowsHaveNoSizes?: boolean
  /**
   * The date column's header in place of the date's own ("Modified"): the Servers list's
   * "Last used", whose rows carry when each server was last used as their date. The header,
   * the measurer, and each row's accessible name read it; the column still sorts by the date.
   */
  dateHeader?: string
  /** What the `..` row says in place of "Parent folder". */
  parentRowName?: string
  /** What an empty list says in place of "Empty folder". */
  emptyText?: string
  /** The footer's count when nothing is selected, in place of "N files and N dirs". */
  countRows?: (count: number) => string
}
