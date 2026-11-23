import { describe, it, expect, beforeAll, afterAll, onTestFinished } from 'vitest'
import { _setLocaleForTests } from '#lib/intl/locale.js'
import { withPlatform } from '#lib/platform/test-platform.js'
import {
  generateDeleteTitle,
  abbreviatePath,
  countSymlinks,
  getSymlinkNotice,
  MAX_VISIBLE_ITEMS,
  type DeleteSourceItem,
} from './delete-dialog-utils'

// Pin the base locale so the catalog-resolved copy these helpers return is the
// deterministic en parity net (these assertions ARE the delete-dialog parity net).
beforeAll(() => {
  _setLocaleForTests('en-US')
})
afterAll(() => {
  _setLocaleForTests(null)
})

function makeItem(overrides: Partial<DeleteSourceItem> = {}): DeleteSourceItem {
  return { name: 'file.txt', size: 1024, isDirectory: false, isSymlink: false, ...overrides }
}

describe('generateDeleteTitle', () => {
  describe('cursor item (isFromCursor = true)', () => {
    it('returns file title for a single file', () => {
      const items = [makeItem()]
      expect(generateDeleteTitle(items, true)).toBe('Delete 1 file under cursor')
    })

    it('returns folder title for a single folder', () => {
      const items = [makeItem({ name: 'components', isDirectory: true })]
      expect(generateDeleteTitle(items, true)).toBe('Delete 1 folder under cursor')
    })
  })

  describe('selected items (isFromCursor = false)', () => {
    it('returns singular file correctly', () => {
      const items = [makeItem()]
      expect(generateDeleteTitle(items, false)).toBe('Delete 1 selected file')
    })

    it('returns plural files correctly', () => {
      const items = [makeItem({ name: 'a.txt' }), makeItem({ name: 'b.txt' })]
      expect(generateDeleteTitle(items, false)).toBe('Delete 2 selected files')
    })

    it('returns singular folder correctly', () => {
      const items = [makeItem({ name: 'src', isDirectory: true })]
      expect(generateDeleteTitle(items, false)).toBe('Delete 1 folder')
    })

    it('returns plural folders correctly', () => {
      const items = [makeItem({ name: 'src', isDirectory: true }), makeItem({ name: 'dist', isDirectory: true })]
      expect(generateDeleteTitle(items, false)).toBe('Delete 2 folders')
    })

    it('combines files and folders', () => {
      const items = [
        makeItem({ name: 'a.txt' }),
        makeItem({ name: 'b.txt' }),
        makeItem({ name: 'c.txt' }),
        makeItem({ name: 'src', isDirectory: true }),
      ]
      expect(generateDeleteTitle(items, false)).toBe('Delete 3 selected files and 1 folder')
    })

    it('returns "Delete" for empty items', () => {
      expect(generateDeleteTitle([], false)).toBe('Delete')
    })
  })

  // Regression (#1432): a link to a folder (a Windows junction, a macOS symlink) reads as a
  // folder in the listing, but only the link goes, and the scan counts it as one file. The
  // title counts it the same way, so the two never disagree.
  describe('a link to a folder', () => {
    const link = makeItem({ name: 'test_junction_1', isDirectory: true, isSymlink: true })

    it('counts as a file under the cursor', () => {
      expect(generateDeleteTitle([link], true)).toBe('Delete 1 file under cursor')
    })

    it('counts as a file in a selection', () => {
      const items = [link, makeItem({ name: 'src', isDirectory: true })]
      expect(generateDeleteTitle(items, false)).toBe('Delete 1 selected file and 1 folder')
    })
  })
})

describe('abbreviatePath', () => {
  it('abbreviates /Users/xxx paths to ~', () => {
    expect(abbreviatePath('/Users/test_user_14/Documents')).toBe('~/Documents')
  })

  it('handles /Users/xxx root', () => {
    expect(abbreviatePath('/Users/test_user_14')).toBe('~')
  })

  it('passes through non-user paths', () => {
    expect(abbreviatePath('/Volumes/NAS/projects')).toBe('/Volumes/NAS/projects')
  })

  it('passes through root path', () => {
    expect(abbreviatePath('/')).toBe('/')
  })
})

describe('countSymlinks', () => {
  it('returns 0 for no symlinks', () => {
    expect(countSymlinks([makeItem(), makeItem({ name: 'dir', isDirectory: true })])).toBe(0)
  })

  it('counts symlinks correctly', () => {
    expect(
      countSymlinks([makeItem({ isSymlink: true }), makeItem(), makeItem({ name: 'link', isSymlink: true })]),
    ).toBe(2)
  })
})

describe('getSymlinkNotice', () => {
  it('returns null when no symlinks', () => {
    expect(getSymlinkNotice([makeItem()])).toBeNull()
  })

  it('returns singular notice for single symlink only item', () => {
    const notice = getSymlinkNotice([makeItem({ isSymlink: true })])
    expect(notice).toBe('This item is a symlink. Only the link will be deleted, not its target.')
  })

  it('returns plural notice for multiple symlinks', () => {
    const notice = getSymlinkNotice([
      makeItem({ isSymlink: true }),
      makeItem(),
      makeItem({ name: 'other', isSymlink: true }),
    ])
    expect(notice).toContain('2 symlinks')
    expect(notice).toContain('links themselves')
  })

  it('returns plural notice for one symlink among many items', () => {
    const notice = getSymlinkNotice([makeItem({ isSymlink: true }), makeItem()])
    expect(notice).toContain('1 symlink')
    expect(notice).toContain('links themselves')
  })

  // Regression (#1432): Windows users meet junctions far more than symlinks, and the listing
  // can't tell the two apart, so Windows names both "link". macOS and Linux keep "symlink".
  describe('per platform', () => {
    it('names a link "link" on Windows, never "symlink"', () => {
      onTestFinished(withPlatform('windows'))
      const single = getSymlinkNotice([makeItem({ isSymlink: true })])
      expect(single).toBe('This item is a link. Only the link will be deleted, not what it points to.')
      const many = getSymlinkNotice([makeItem({ isSymlink: true }), makeItem(), makeItem({ isSymlink: true })])
      expect(many).toBe(
        'Your selection includes 2 links. Only the links themselves will be deleted, not what they point to.',
      )
    })

    it.each(['macos', 'linux'] as const)('keeps "symlink" on %s', (platform) => {
      onTestFinished(withPlatform(platform))
      expect(getSymlinkNotice([makeItem({ isSymlink: true })])).toBe(
        'This item is a symlink. Only the link will be deleted, not its target.',
      )
      expect(getSymlinkNotice([makeItem({ isSymlink: true }), makeItem()])).toContain('1 symlink')
    })
  })
})

describe('MAX_VISIBLE_ITEMS', () => {
  it('is 10', () => {
    expect(MAX_VISIBLE_ITEMS).toBe(10)
  })
})
