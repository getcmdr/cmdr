/**
 * A share list's rows carry no dates and no sizes (the `network` volume is unindexed and a
 * share row's path is no filesystem path), so its kind words drop both columns. The Servers
 * list keeps its date as "Last used", heads its Ext column "Details", and its footer counts
 * servers with the nearby search's state (or says networking is off). Every other kind keeps a folder's own words.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const discovery = vi.hoisted((): { state: string; enabled: boolean } => ({ state: 'idle', enabled: true }))
vi.mock('../network/network-store.svelte', () => ({ getDiscoveryState: () => discovery.state }))
vi.mock('#lib/settings/reactive-settings.svelte.js', () => ({ getNetworkEnabled: () => discovery.enabled }))

import { functionKeyWordsFor, listWordsFor, trailingRowFor } from './list-kind-words'

describe('listWordsFor', () => {
  beforeEach(() => {
    discovery.state = 'idle'
    discovery.enabled = true
  })

  it('says a share list’s rows carry no dates and no sizes', () => {
    const words = listWordsFor('share-list')
    expect(words?.rowsHaveNoDates).toBe(true)
    expect(words?.rowsHaveNoSizes).toBe(true)
  })

  it('gives the Servers list Details and Last used, and no Size column', () => {
    const words = listWordsFor('network')
    expect(words?.extHeader).toBe('Details')
    expect(words?.extIsProse).toBe(true)
    expect(words?.dateHeader).toBe('Last used')
    expect(words?.rowsHaveNoDates).toBeUndefined()
    expect(words?.rowsHaveNoSizes).toBe(true)
    expect(words?.emptyText).toContain('F7')
  })

  it('counts servers, saying when the nearby search is still looking or networking is off', () => {
    expect(listWordsFor('network')?.countRows?.(1)).toBe('1 server')
    discovery.state = 'searching'
    expect(listWordsFor('network')?.countRows?.(1_200)).toBe('1,200 servers, looking for more nearby…')
    discovery.enabled = false
    expect(listWordsFor('network')?.countRows?.(5)).toBe('5 servers, networking is off')
  })

  it('gives a folder on any other kind its own words', () => {
    expect(listWordsFor('local')).toBeUndefined()
    expect(listWordsFor('smb')).toBeUndefined()
  })
})

describe('functionKeyWordsFor', () => {
  it('makes F7 Add server and F8 Forget on the Servers list, both enabled, and F2 Edit server', () => {
    const keys = functionKeyWordsFor('network')
    expect(keys.newFolder).toEqual({ label: 'Add server', enabled: true })
    expect(keys.delete).toEqual({ label: 'Forget', enabled: true })
    expect(keys.renameEnabled).toBe(true)
  })

  it('keeps F7 and F8 a folder’s own on a share list (they refuse there), with F2 Edit server', () => {
    const keys = functionKeyWordsFor('share-list')
    expect(keys.newFolder).toBeUndefined()
    expect(keys.delete).toBeUndefined()
    expect(keys.renameEnabled).toBe(true)
  })

  it('leaves every key a folder’s own elsewhere', () => {
    expect(functionKeyWordsFor('local')).toEqual({})
  })
})

describe('trailingRowFor', () => {
  it('ends the Servers list in "Add server…", which opens the Add server sheet', () => {
    expect(trailingRowFor('network')).toEqual({
      label: 'Add server…',
      commandId: 'servers.connect',
      mcpName: '+ Add server…',
    })
  })

  it('gives every other kind no trailing row, a share list included', () => {
    expect(trailingRowFor('share-list')).toBeUndefined()
    expect(trailingRowFor('local')).toBeUndefined()
    expect(trailingRowFor('search-results')).toBeUndefined()
  })
})
