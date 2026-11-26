/**
 * Which parts of the pane's status footer a given view gets.
 *
 * Two independent answers, because a search-results pane needs one and not the
 * other:
 * - `selectionInfo`: the row counts and the selection summary. A snapshot pane
 *   wants them as much as a folder does — "how many hits, how big are the ones I
 *   picked" is the whole question a search answers.
 * - `volumeSpace`: the free-space text and the usage bar above it. A snapshot
 *   isn't a place on a disk: its rows all live on ONE volume, but the pane isn't
 *   sitting IN that volume, so a free-space figure there would answer a question
 *   nobody asked and would follow the user around as they walk history.
 *
 * A server's share list renders as a normal list but sits on no disk either: the
 * `network` volume never answers a space query, so its usage bar would be the
 * "not yet fetched" empty track forever, which reads as "0% full". It gets the
 * counts and no space, from the pane's routed KIND (❌ never a `volumeId` check).
 *
 * The network view has no listing to count, and an error state replaces the list
 * entirely, so both lose the footer completely.
 */

import type { PaneViewKind } from './types'
import type { VolumeKind } from './volume-capabilities'

export interface PaneFooterInput {
  kind: PaneViewKind
  /** The pane's routed kind (`capabilitiesForPane`): what decides whether there's a disk to talk about. */
  volumeKind: VolumeKind
  /** Whether the pane is showing an error or unreachable state instead of a list. */
  hasError: boolean
}

export interface PaneFooterVisibility {
  /** Render `SelectionInfo` under the list. */
  selectionInfo: boolean
  /** Let that readout talk about disk space, and render the usage bar above it. */
  volumeSpace: boolean
}

/** Resolves which footer pieces a pane view renders. */
export function paneFooterVisibility({ kind, volumeKind, hasError }: PaneFooterInput): PaneFooterVisibility {
  if (hasError) return { selectionInfo: false, volumeSpace: false }
  if (kind === 'normal') return { selectionInfo: true, volumeSpace: volumeKind !== 'share-list' }
  if (kind === 'search-results') return { selectionInfo: true, volumeSpace: false }
  return { selectionInfo: false, volumeSpace: false }
}
