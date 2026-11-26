/**
 * What a pane does once the share list a door sent it to lands: mount a share the door
 * named (an `smb://host/share` address, a saved share, "Copy path between panes" with the
 * cursor on a share), put the cursor on the share `..` came out of (#1238), and whether a
 * sign-in refusal on the way asks for an account on its own (the person opened it).
 *
 * Every door that opens a server's share list goes through {@link queueShareListArrival},
 * then navigates the pane there: an SMB server's Servers row (`share-list-pane.svelte.ts::openShare`),
 * the add-server hand-off, the mirror, and `..` out of a share's top folder. ❌ No door threads a
 * share through the navigation.
 *
 * ❗ **A one-shot for the visit it was queued with** (cmdr-reports#7: "Cmdr tried to open
 * the share on that server"). Kept per PANE, ❌ not on a pane component: a pinned tab
 * opens the share list in a NEW tab, whose pane is a fresh component. It's spent once the
 * share list lands (`takeShareListArrival`), and dropped the moment the pane is anywhere
 * but where it was queued from or the share list it was queued for
 * (`dropShareListArrivalOff`), so a refused navigation, Back, or another server never
 * carries it to a later visit that nobody pointed at that share.
 */

import { sameShareList, shareListPath } from '#lib/servers/share-list-path.js'
import type { ShareParentLanding } from '../network/share-parent-row.svelte'
import type { NetworkHost } from '../types'

/** What a door asked for, for the one share list it sent the pane to. */
export interface ShareListArrival {
  /** The share-list path the door sent the pane to. */
  path: string
  /** Where the pane was when the door queued it: the one other place it may still be. */
  origin: string
  /** A share to mount once the list lands. */
  mount?: string
  /** The share `..` came out of: the cursor lands on it, and the hint explains where `..` went. */
  land?: ShareParentLanding
  /**
   * Set once a sign-in refusal on the way asked for an account on its own: the person
   * opened this share list, so its refusal asks, ❗ once. A repaint, a tab coming back,
   * or any re-list nobody started finds no arrival, or this set, and asks nothing.
   */
  asked?: boolean
}

const arrivals = $state<Record<'left' | 'right', ShareListArrival | null>>({ left: null, right: null })

/** Queues what the pane does once the share list it's about to be sent to lands. */
export function queueShareListArrival(pane: 'left' | 'right', arrival: ShareListArrival): void {
  arrivals[pane] = arrival
}

/**
 * A door opening `host`'s share list in `pane`: queues its arrival and answers the path
 * to send the pane to. `origin` is where the pane is now.
 */
export function openShareListArrival(
  pane: 'left' | 'right',
  host: NetworkHost,
  arrival: { origin: string; mount?: string; land?: ShareParentLanding },
): string {
  const path = shareListPath(host)
  queueShareListArrival(pane, { path, ...arrival })
  return path
}

/** The pane's queued arrival, without spending it. Reactive. */
export function shareListArrivalFor(pane: 'left' | 'right'): ShareListArrival | null {
  return arrivals[pane]
}

/**
 * Claims the pane's one sign-in ask for the share list at `path`: `true` the first time a
 * refusal there may open the sheet on its own, `false` with no arrival for it or once asked.
 */
export function claimShareListAsk(pane: 'left' | 'right', path: string): boolean {
  const arrival = arrivals[pane]
  if (!arrival || arrival.asked === true || !sameShareList(arrival.path, path)) return false
  arrival.asked = true
  return true
}

/** Spends the pane's arrival for the share list at `path`, or answers `null` when none waits for it. */
export function takeShareListArrival(pane: 'left' | 'right', path: string): ShareListArrival | null {
  const arrival = arrivals[pane]
  if (!arrival || !sameShareList(arrival.path, path)) return null
  arrivals[pane] = null
  return arrival
}

/** Drops the pane's arrival once the pane is at `path`, when that's neither where it was queued from nor for. */
export function dropShareListArrivalOff(pane: 'left' | 'right', path: string): void {
  const arrival = arrivals[pane]
  if (!arrival || path === arrival.origin || sameShareList(arrival.path, path)) return
  arrivals[pane] = null
}
