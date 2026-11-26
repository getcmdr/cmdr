/**
 * A share list's rows carry no dates and no sizes (the `network` volume is unindexed and a
 * share row's path is no filesystem path), so its kind words drop both columns. Every other
 * kind keeps a folder's own words, Size and dates included.
 */
import { describe, it, expect } from 'vitest'
import { listWordsFor } from './list-kind-words'

describe('listWordsFor', () => {
  it('says a share list’s rows carry no dates and no sizes', () => {
    const words = listWordsFor('share-list')
    expect(words?.rowsHaveNoDates).toBe(true)
    expect(words?.rowsHaveNoSizes).toBe(true)
  })

  it('gives a folder on any other kind its own words', () => {
    expect(listWordsFor('local')).toBeUndefined()
    expect(listWordsFor('smb')).toBeUndefined()
  })
})
