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
  /** What the `..` row says in place of "Parent folder". */
  parentRowName?: string
  /** What an empty list says in place of "Empty folder". */
  emptyText?: string
  /** The footer's count when nothing is selected, in place of "N files and N dirs". */
  countRows?: (count: number) => string
}
