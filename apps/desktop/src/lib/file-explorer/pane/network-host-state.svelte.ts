/**
 * The Network host a pane has open, as history entries still carry it. A server's share
 * list is a path now (`smb://<server>`), so nothing sets a host here any more: what's left
 * is the history plumbing that retires with it.
 *
 * Guardrail: leaving the network volume by ANY route clears it, so re-entering Network
 * always lands on the host list.
 */

import type { NetworkHost } from '../types'

export interface NetworkHostStateDeps {
  getIsNetworkView: () => boolean
  /** Bubble a host change out of the pane (history tracking). */
  onHostChange: (host: NetworkHost | null) => void
}

export interface NetworkHostState {
  /** The host the pane's history entry names, or null for the host list. */
  readonly host: NetworkHost | null
  /** Set the host without bubbling (the pane API's `setNetworkHost`). */
  setHost: (host: NetworkHost | null) => void
  /** Remember a host change AND bubble it. */
  handleHostChange: (host: NetworkHost | null) => void
}

export function createNetworkHostState(deps: NetworkHostStateDeps): NetworkHostState {
  let host = $state<NetworkHost | null>(null)

  $effect(() => {
    if (deps.getIsNetworkView()) return
    if (host !== null) host = null
  })

  return {
    get host() {
      return host
    },
    setHost: (next) => {
      host = next
    },
    handleHostChange: (next) => {
      host = next
      deps.onHostChange(next)
    },
  }
}
