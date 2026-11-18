/**
 * The escape's frontend half, driven by the `escapes` rows of the case table Rust's suite
 * reads too (`crates/cmdr-fs/src/volume/path_grammar_cases.json`). A new case goes into that
 * file, ❌ never into one suite.
 */
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it, onTestFinished } from 'vitest'
import { withPlatform } from '#lib/platform/test-platform.js'
import { copiedPath, displayPath } from '#lib/utils/display-path.js'
import { isEscape, nativeNameLength, shownName, withoutEscapes } from './escape'
import { basenameInVolume, joinPath, parentInVolume, type PathFlavor } from './grammar'

interface EscapeCase {
  path: string
  flavor: PathFlavor
  parent: string | null
  basename: string | null
  joinName: string | null
  joined: string | null
  shown: string | null
  shownName: string | null
}

const table = JSON.parse(
  readFileSync(resolve(process.cwd(), '../../crates/cmdr-fs/src/volume/path_grammar_cases.json'), 'utf8'),
) as { escapes: EscapeCase[] }

/** Rows the frontend answers: the canonical ones (it never respells what a backend sent). */
const frontendRows = table.escapes.filter((c) => c.parent !== null)

describe('the shared escape rows', () => {
  it('has rows to run', () => {
    expect(frontendRows.length).toBeGreaterThan(0)
  })

  it.each(frontendRows.map((c) => [`${c.flavor} ${JSON.stringify(c.path)}`, c] as const))('%s', (_name, c) => {
    expect(parentInVolume(c.path, c.flavor)).toBe(c.parent)
    expect(basenameInVolume(c.path, c.flavor)).toBe(c.basename)
    if (c.joinName !== null) expect(joinPath(c.path, c.joinName)).toBe(c.joined)
    if (c.shown !== null) {
      onTestFinished(withPlatform(c.flavor === 'windows' ? 'windows' : 'macos'))
      expect(displayPath(c.path)).toBe(c.shown)
    }
    if (c.shownName !== null && c.basename !== null) expect(shownName(c.basename)).toBe(c.shownName)
  })
})

describe('withoutEscapes', () => {
  it('shows every escape as U+FFFD, in any text', () => {
    expect(withoutEscapes('caf\u{10FEE9}.txt')).toBe('caf�.txt')
    expect(withoutEscapes('a\u{10E800}b')).toBe('a�b')
    expect(withoutEscapes('Delete “caf\u{10FEE9}”?')).toBe('Delete “caf�”?')
  })

  it('leaves the rest of plane 16 and every emoji alone', () => {
    for (const text of ['\u{10FE7F}', '\u{10F000}', '\u{100000}', 'photo 😀.jpg', '\u{F0001}', '']) {
      expect(withoutEscapes(text)).toBe(text)
    }
  })

  it('knows both alphabets by their edges', () => {
    expect([0x10fe80, 0x10feff, 0x10e800, 0x10efff].every(isEscape)).toBe(true)
    expect([0x10fe7f, 0x10ff00, 0x10e7ff, 0x10f000].some(isEscape)).toBe(false)
  })
})

describe('displayPath and copiedPath', () => {
  it('never show or copy a raw escape', () => {
    expect(displayPath('/d/caf\u{10FEE9}')).toBe('/d/caf�')
    expect(copiedPath('/d/caf\u{10FEE9}')).toBe('/d/caf�')
  })
})

describe('nativeNameLength', () => {
  it('counts an escape as the one byte or unit it stands for', () => {
    expect(nativeNameLength('caf\u{10FEE9}', 'bytes')).toBe(4)
    expect(nativeNameLength('a\u{10E800}b', 'units')).toBe(3)
    expect(new TextEncoder().encode('caf\u{10FEE9}').length).toBe(7)
  })

  it('counts everything else in its encoding', () => {
    expect(nativeNameLength('café', 'bytes')).toBe(5)
    expect(nativeNameLength('café', 'units')).toBe(4)
    expect(nativeNameLength('😀', 'bytes')).toBe(4)
    expect(nativeNameLength('😀', 'units')).toBe(2)
  })
})
