/**
 * Tests for the OCR text matcher. The cases are the shapes the 2026-10-31 measurement
 * tuned it on: OCR slips, wrapped text, plurals, placeholders, per-OS blocks, raw
 * markdown, and the near-misses that must NOT count as found.
 */

import { describe, it, expect } from 'vitest'
import { normalizeOcrText, ocrDoc, requirementOf, runShows, showsText } from './screenshot-text-match.ts'

const doc = (...lines: string[]) => ocrDoc(lines)

describe('normalizeOcrText', () => {
  it('folds quotes, dashes, ellipses, and OCR look-alikes', () => {
    expect(normalizeOcrText('Don’t – “Save”…')).toBe(normalizeOcrText('Don\'t - "Save"...'))
    expect(normalizeOcrText('Fi1e l0g')).toBe(normalizeOcrText('File log'))
  })

  it('folds Cyrillic look-alikes OCR sometimes returns', () => {
    expect(normalizeOcrText('Аpp')).toBe(normalizeOcrText('App'))
  })
})

describe('runShows', () => {
  it('finds a long run with an OCR slip in it', () => {
    const d = doc('Minimum change in avai1able disk space before updating the status bar.')
    expect(runShows(normalizeOcrText('minimum change in available disk space before updating'), d)).toBe(true)
  })

  it('finds a long run the window wrapped across lines', () => {
    const d = doc('Delete everything Cmdr keeps on this', 'Mac: settings, saved passwords')
    expect(runShows(normalizeOcrText('Delete everything Cmdr keeps on this Mac: settings, saved passwords'), d)).toBe(
      true,
    )
  })

  it('wants a short run word-aligned, not inside a longer word', () => {
    expect(runShows(normalizeOcrText('Pause'), doc('Paused'))).toBe(false)
    expect(runShows(normalizeOcrText('Pause'), doc('Pause', 'Cancel'))).toBe(true)
  })

  it("doesn't take an inflection for an OCR slip", () => {
    expect(runShows(normalizeOcrText('Server'), doc('Servers'))).toBe(false)
  })

  it('misses text the picture lacks', () => {
    expect(runShows(normalizeOcrText('Drag threshold'), doc('Verbose console output', 'Log AI model calls'))).toBe(
      false,
    )
  })
})

describe('showsText', () => {
  it('checks the literal runs around a placeholder', () => {
    const english = 'Busy folders get a look within {hot}, quieter ones within {warm}.'
    expect(
      showsText('settings.x.summary', english, doc('Busy folders get a look within 5 min, quieter ones within 1 h.')),
    ).toBe(true)
    expect(showsText('settings.x.summary', english, doc('Busy folders get a look'))).toBe(false)
  })

  it('accepts any plural branch the picture shows', () => {
    const english = '{count, plural, one {{countText} file left} other {{countText} files left}}'
    expect(showsText('transfer.x.filesLeft', english, doc('3 files left'))).toBe(true)
    expect(showsText('transfer.x.filesLeft', english, doc('1 file left'))).toBe(true)
    expect(showsText('transfer.x.filesLeft', english, doc('Pause', 'Cancel'))).toBe(false)
  })

  it('joins a tag into the text around it', () => {
    expect(showsText('askCmdr.x.doing', 'Listing <place>{name}</place> now', doc('Listing left now'))).toBe(true)
  })

  it('reads the macOS branch of a per-OS block', () => {
    const english = '{platform, select, macos {Show in Finder} windows {Show in Explorer} other {Show in files}}'
    expect(showsText('settings.x.reveal', english, doc('Show in Finder'))).toBe(true)
  })

  it('reads a raw value past its markdown and tokens', () => {
    const english = '- Open **{system_settings} > Network** and check your connection\n- Run `ping <hostname>`'
    const picture = doc('Open System Settings > Network and check your connection', 'Run ping <hostname>')
    expect(showsText('errors.listing.x.suggestion', english, picture)).toBe(true)
  })

  it('checks a short-only message exactly, and answers null when there is no text at all', () => {
    expect(showsText('common.sizeUnit.gibibyte', 'GiB', doc('12 GiB free'))).toBe(true)
    expect(showsText('common.sizeUnit.gibibyte', 'GiB', doc('12 MiB free'))).toBe(false)
    expect(showsText('fileExplorer.x.name', '{name}', doc('anything'))).toBeNull()
  })
})

describe('requirementOf', () => {
  it('breaks the English at each placeholder and branches at each plural', () => {
    expect(requirementOf('a.b', 'Rename {countText} {count, plural, one {file} other {files}}')).toEqual({
      kind: 'all',
      children: [
        { kind: 'literal', text: 'Rename ' },
        { kind: 'literal', text: ' ' },
        {
          kind: 'any',
          children: [
            { kind: 'all', children: [{ kind: 'literal', text: 'file' }] },
            { kind: 'all', children: [{ kind: 'literal', text: 'files' }] },
          ],
        },
      ],
    })
  })
})
