/**
 * Tests for the cache rules of the tracked OCR text: which images get read again, and
 * what the next file keeps.
 */

import { describe, it, expect } from 'vitest'
import { imagesToRead, mergeOcrText, type OcrText } from './screenshot-ocr.ts'

const existing: OcrText = {
  'a.png': { sha256: 'aaa', lines: ['A'] },
  'b.png': { sha256: 'bbb', lines: ['B'] },
  'gone.png': { sha256: 'ggg', lines: ['G'] },
}

describe('imagesToRead', () => {
  it('reads only images whose hash moved or that have no entry', () => {
    const onDisk: Record<string, string> = { 'a.png': 'aaa', 'b.png': 'b2', 'c.png': 'ccc' }
    const read = imagesToRead(['a.png', 'b.png', 'c.png'], existing, (file) => onDisk[file] ?? null)
    expect(read).toEqual([
      { file: 'b.png', sha256: 'b2' },
      { file: 'c.png', sha256: 'ccc' },
    ])
  })

  it('reads nothing when the images are not on disk (CI, or a checkout without PNGs)', () => {
    expect(imagesToRead(['a.png', 'z.png'], existing, () => null)).toEqual([])
  })
})

describe('mergeOcrText', () => {
  it('keeps unchanged entries, takes fresh ones, drops what the report no longer names, and sorts', () => {
    const merged = mergeOcrText(['b.png', 'a.png'], existing, { 'b.png': { sha256: 'b2', lines: ['B2'] } })
    expect(Object.keys(merged)).toEqual(['a.png', 'b.png'])
    expect(merged['a.png']).toEqual(existing['a.png'])
    expect(merged['b.png']).toEqual({ sha256: 'b2', lines: ['B2'] })
  })
})
