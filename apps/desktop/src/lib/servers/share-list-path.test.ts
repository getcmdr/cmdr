/**
 * The share-list path's writer and reader, over the case table Rust reads too
 * (`share-list-path-cases.json`, through `src-tauri/src/network/share_list_path_test.rs`).
 * A new case goes into that table, so both languages keep spelling one server one way.
 */
import { describe, expect, it } from 'vitest'
import type { NetworkHost } from '#lib/file-explorer/types.js'
import { basenameInVolume, parentInVolume } from '#lib/path/grammar.js'
import cases from './share-list-path-cases.json'
import { sameShareList, shareListHostKey, shareListPath } from './share-list-path'

const HOSTS = cases.hosts as { why: string; host: NetworkHost; path: string; key: string; port: number }[]

describe('shareListPath', () => {
  it.each(HOSTS)('$why', ({ host, path }) => {
    expect(shareListPath(host)).toBe(path)
  })
})

describe('sameShareList', () => {
  it('folds case, an explicit 445, and the trailing slash Rust spells a root with', () => {
    expect(sameShareList('smb://TestNAS', 'smb://testnas:445/')).toBe(true)
  })

  it('tells two servers, and two ports of one server, apart', () => {
    expect(sameShareList('smb://testnas', 'smb://testnas2')).toBe(false)
    expect(sameShareList('smb://testnas', 'smb://testnas:4450')).toBe(false)
  })

  it('names no share list for the hub or a share', () => {
    expect(sameShareList('smb://', 'smb://')).toBe(false)
    expect(sameShareList('smb://testnas/public', 'smb://testnas/public')).toBe(false)
  })
})

describe('shareListHostKey', () => {
  it.each(HOSTS)('reads back what the writer wrote: $why', ({ host, key, port }) => {
    expect(shareListHostKey(shareListPath(host))).toEqual({ key, port })
  })

  it.each(cases.spellings)('$why', ({ path, key, port }) => {
    expect(shareListHostKey(path)).toEqual({ key, port })
  })

  it.each(cases.notShareLists)('$why is no share list', ({ path }) => {
    expect(shareListHostKey(path)).toBeNull()
  })
})

/** ❗ The path grammar reads the whole authority as one token: a share under it splits off as its own name. */
describe('the path grammar', () => {
  it.each(HOSTS)('keeps the authority whole: $why', ({ host }) => {
    const shareList = shareListPath(host)
    expect(basenameInVolume(`${shareList}/archive`)).toBe('archive')
    expect(parentInVolume(`${shareList}/archive`)).toBe(shareList)
  })
})
