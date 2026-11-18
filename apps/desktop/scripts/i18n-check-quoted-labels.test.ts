/**
 * Tests for the QUOTED-LABEL check (`i18n-check-quoted-labels.ts`): text that
 * quotes another key's label must carry that label as the locale writes it.
 */
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, rmSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  quotedLabelPairs,
  mismatchedQuotes,
  quoteMismatches,
  applyQuoteExemptions,
  runQuotedLabelsCheck,
} from './i18n-check-quoted-labels.ts'
import { EXIT_CLEAN, EXIT_ISSUES } from './i18n-locale-check-lib.ts'

const en = {
  'a.dontShow': 'Don’t show again',
  'a.reopen': 'Reopen closed tab…',
  'a.hint': 'Click “Don’t show again” to hide it for good.',
  'a.menu': 'Use “Reopen closed tab” (⌘⇧T) to bring it back.',
  'a.word': 'Type “hello” to test, or look under “added”.',
  'a.added': 'added',
  'a.long': 'This is a long sentence that nobody quotes anywhere else at all.',
}

describe('quotedLabelPairs', () => {
  it('pairs English text with the keys whose whole value it quotes, ignoring a trailing ellipsis', () => {
    expect(quotedLabelPairs(en)).toEqual([
      { key: 'a.hint', label: 'Don’t show again', sources: ['a.dontShow'] },
      { key: 'a.menu', label: 'Reopen closed tab', sources: ['a.reopen'] },
    ])
  })
})

describe('mismatchedQuotes', () => {
  const pairs = quotedLabelPairs(en)
  it('flags a quote that paraphrases the label, and passes one that copies it in any case or quote style', () => {
    const es = {
      'a.dontShow': 'No volver a mostrar',
      'a.reopen': 'Reabrir la pestaña cerrada…',
      'a.hint': 'Haz clic en “No mostrar más” para ocultarlo.',
      'a.menu': 'Usa «reabrir la pestaña cerrada» (⌘⇧T).',
    }
    expect(mismatchedQuotes(pairs, es)).toEqual([
      {
        key: 'a.hint',
        detail: 'quotes “Don’t show again”, but not its label here: “No volver a mostrar” (a.dontShow)',
      },
    ])
  })
  it('skips a pair whose key or label the locale lacks', () => {
    expect(mismatchedQuotes(pairs, { 'a.hint': 'x' })).toEqual([])
  })
})

describe('platform blocks, compared per rendered OS', () => {
  const enBlocks = {
    'b.reveal':
      '{platform, select, macos {Show in Finder} windows {Show in File Explorer} other {Show in file manager}}',
    'b.hint':
      '{platform, select, macos {Pick “Show in Finder” to see it.} windows {Pick “Show in File Explorer” to see it.} other {Pick “Show in file manager” to see it.}}',
  }

  it('pairs each OS’s quote with the label that OS shows', () => {
    expect(quotedLabelPairs(enBlocks)).toEqual([
      { key: 'b.hint', label: 'Show in Finder', sources: ['b.reveal'], platforms: ['macos'] },
      { key: 'b.hint', label: 'Show in File Explorer', sources: ['b.reveal'], platforms: ['windows'] },
      { key: 'b.hint', label: 'Show in file manager', sources: ['b.reveal'], platforms: ['linux'] },
    ])
  })

  it('judges a translated rendering, and skips an OS whose either half still carries its English', () => {
    const de = {
      'b.reveal':
        '{platform, select, macos {Im Finder zeigen} windows {Show in File Explorer} other {Im Dateimanager zeigen}}',
      'b.hint':
        '{platform, select, macos {Wähle „Im Finder anzeigen“.} windows {Wähle „Im Explorer zeigen“.} other {Pick “Show in file manager” to see it.}}',
    }
    expect(mismatchedQuotes(quotedLabelPairs(enBlocks), de, enBlocks)).toEqual([
      {
        key: 'b.hint',
        detail: 'quotes “Show in Finder”, but not its label here: “Im Finder zeigen” (b.reveal) on macos',
      },
    ])
  })
})

describe('applyQuoteExemptions: a quote naming another app’s own label, on one OS', () => {
  const enOs = {
    'c.readOnly': 'Read-only',
    'c.fix':
      '{platform, select, macos {Unlock it in Finder.} windows {Clear “Read-only” in File Explorer.} other {Check its permissions.}}',
  }
  const cs = {
    'c.readOnly': 'Jen ke čtení',
    'c.fix':
      '{platform, select, macos {Odemkněte ho ve Finderu.} windows {V Průzkumníku zrušte „Jen pro čtení“.} other {Zkontrolujte oprávnění.}}',
  }
  const mismatches = quoteMismatches(quotedLabelPairs(enOs), cs, enOs)
  const entry = {
    key: 'c.fix',
    platform: 'windows',
    label: 'Read-only',
    reason: 'File Explorer’s own checkbox, „Jen pro čtení“ (Microsoft termbase).',
  }

  it('silences the one mismatch an entry names, with nothing stale', () => {
    expect(mismatches.map(({ key, platform }) => [key, platform])).toEqual([['c.fix', 'windows']])
    expect(applyQuoteExemptions(mismatches, [entry])).toEqual({ issues: [], stale: [] })
  })

  it('doesn’t count an entry with a blank reason: the reason is the point', () => {
    const blank = { ...entry, reason: '  ' }
    const { issues, stale } = applyQuoteExemptions(mismatches, [blank])
    expect(issues.map(({ key }) => key)).toEqual(['c.fix', 'c.fix'])
    expect(issues[1].detail).toMatch(/exemption for “Read-only” on windows has no reason/)
    expect(stale).toEqual([])
  })

  it('doesn’t reach another OS branch or another quoted label', () => {
    const elsewhere = [
      { ...entry, platform: 'linux' },
      { ...entry, label: 'Hidden' },
    ]
    const { issues, stale } = applyQuoteExemptions(mismatches, elsewhere)
    expect(issues).toHaveLength(1)
    expect(stale).toEqual(elsewhere)
  })

  it('flags an entry whose quote no longer mismatches, so the allowlist shrinks', () => {
    const fixed = { ...cs, 'c.fix': cs['c.fix'].replace('„Jen pro čtení“', '„Jen ke čtení“') }
    const { issues, stale } = applyQuoteExemptions(quoteMismatches(quotedLabelPairs(enOs), fixed, enOs), [entry])
    expect(issues).toEqual([])
    expect(stale).toEqual([entry])
  })
})

describe('runQuotedLabelsCheck (fixture tree)', () => {
  let root: string
  const write = (path: string, data: unknown) => {
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, JSON.stringify(data, null, 2))
  }
  beforeEach(() => {
    root = mkdtempSync(join(tmpdir(), 'quoted-labels-'))
    write(join(root, 'en', 'a.json'), en)
    write(join(root, 'es', 'a.json'), {
      ...en,
      'a.dontShow': 'No volver a mostrar',
      'a.hint': 'Pulsa “No volver a mostrar”.',
    })
    write(join(root, 'es-419', 'a.json'), { 'a.dontShow': 'No mostrar de nuevo' })
  })
  afterEach(() => {
    rmSync(root, { recursive: true, force: true })
  })

  it('reads an overlay through its base: a forked label the quoting text still spells the old way is a finding', () => {
    const lines: string[] = []
    expect(runQuotedLabelsCheck({ messagesRoot: root, write: (line) => void lines.push(line) })).toBe(EXIT_ISSUES)
    expect(lines.join('\n')).toMatch(/es: clean/)
    expect(lines.join('\n')).toMatch(/es-419[\s\S]*a\.hint → quotes “Don’t show again”/)
  })

  it('skips the pseudolocale even when named: its per-string padding never matches inside the quoting text', () => {
    write(join(root, 'en-XA', 'a.json'), {
      'a.dontShow': 'Ðöñ’ţ šḣöŵ áǧáíñ⟦áéíöü⟧',
      'a.hint': 'Ṗŕéšš “Ðöñ’ţ šḣöŵ áǧáíñ”.⟦áéíöüáé⟧',
    })
    const lines: string[] = []
    const code = runQuotedLabelsCheck({ messagesRoot: root, write: (line) => void lines.push(line), only: ['en-XA'] })
    expect(code).toBe(EXIT_CLEAN)
    expect(lines.join('\n')).toMatch(/en-XA: skipped/)
  })

  it('reads the allowlist: a reasoned entry silences its finding, and a stale one is a finding', () => {
    const allowlistPath = join(root, 'allowlist.json')
    const exempt = (key: string) => ({
      otherAppLabels: { 'es-419': [{ key, platform: 'linux', label: 'Don’t show again', reason: 'Test reason.' }] },
    })
    // The fixture's English is plain, so its pairs carry no platform: an entry can only name an OS branch.
    write(allowlistPath, exempt('a.hint'))
    const lines: string[] = []
    expect(runQuotedLabelsCheck({ messagesRoot: root, allowlistPath, write: (line) => void lines.push(line) })).toBe(
      EXIT_ISSUES,
    )
    expect(lines.join('\n')).toMatch(/a\.hint → stale exemption: “Don’t show again” on linux no longer mismatches/)
    expect(lines.join('\n')).toMatch(/a\.hint → quotes “Don’t show again”/)
  })

  it('is clean once the overlay forks the quoting text too', () => {
    write(join(root, 'es-419', 'a.json'), {
      'a.dontShow': 'No mostrar de nuevo',
      'a.hint': 'Pulsa “No mostrar de nuevo”.',
    })
    expect(runQuotedLabelsCheck({ messagesRoot: root, write: () => {} })).toBe(EXIT_CLEAN)
  })
})
