/**
 * Whether a screenshot's OCR text shows a catalog key's English: the pure core of the
 * coupler's OCR check (`couple-screenshots.ts`).
 *
 * A key's coupling used to be the first surface that LOOKED ITS TEXT UP, which isn't the
 * same as one that shows it: a menu resolves its rows while closed, a settings section
 * resolves every row below the fold, a tool line resolves its running words and is
 * photographed finished. An OCR pass over the 2026-10-31 images found a third of the
 * coupled keys missing from their own image. This module answers the question the
 * coupling actually needs: is this text in this picture?
 *
 * The English becomes a requirement tree: literal runs (AND), with each `plural` /
 * `select` an OR of its branches, each `{placeholder}` a break between runs, and a tag's
 * text joined into the run around it. A run shorter than three characters isn't checked
 * (it matches some other element by chance); a message with nothing longer checks its
 * short runs exactly. Matching tolerates OCR: normalized quotes and dashes, folded
 * look-alikes (`l`/`1`/`I`, `0`/`O`), a small edit distance on longer runs, and word
 * coverage for text the window wrapped or split into columns. Tuned on the 2026-10-31
 * set (about 90–95 % agreement with eyeballing; the misses are short common words that
 * match another element, and garbled lines a frame cuts off).
 *
 * Pure: no filesystem, no OCR. `screenshot-ocr.ts` produces the text.
 */

import { IntlMessageFormat } from 'intl-messageformat'
import { pickPlatformBranch } from '../src/lib/intl/platform-select.ts'
import { TYPE, isRawKey } from './i18n-catalog-lib.ts'

/** A requirement: a literal run, all of several, or any one of several. */
export type Requirement =
  | { kind: 'literal'; text: string }
  | { kind: 'all'; children: Requirement[] }
  | { kind: 'any'; children: Requirement[] }

/** The subset of the ICU AST this walk reads. */
interface IcuElement {
  type: number
  value?: string
  options?: Record<string, { value: IcuElement[] }>
  children?: IcuElement[]
}

/** Builds AND-of-runs from ICU elements, joining literal text across tags. */
function fromIcu(elements: readonly IcuElement[]): Requirement {
  const children: Requirement[] = []
  let run = ''
  const flush = (): void => {
    if (run !== '') children.push({ kind: 'literal', text: run })
    run = ''
  }
  const walk = (list: readonly IcuElement[]): void => {
    for (const el of list) {
      if (el.type === TYPE.literal) run += el.value ?? ''
      else if (el.type === TYPE.tag) walk(el.children ?? [])
      else if (el.type === TYPE.select || el.type === TYPE.plural) {
        flush()
        children.push({ kind: 'any', children: Object.values(el.options ?? {}).map((o) => fromIcu(o.value)) })
      } else flush() // an argument, a number, a date, `#`: text we can't predict
    }
  }
  walk(elements)
  flush()
  return { kind: 'all', children }
}

/** A raw (`errors.*`, native) value: markdown and `{token}`s around literal text. */
function fromRaw(value: string): Requirement {
  const text = value
    .replaceAll(/\[([^\]]*)\]\([^)]*\)/g, '$1') // a link shows its text
    .replaceAll(/\*\*|`/g, '')
    .replaceAll(/^\s*[-*]\s+/gm, '') // bullet markers aren't text
  return {
    kind: 'all',
    children: text
      .split(/\{[^{}]*\}|\n/)
      .filter((part) => part.trim() !== '')
      .map((part) => ({ kind: 'literal', text: part })),
  }
}

/**
 * What a key's English requires a picture to show, as macOS renders it (the capture's OS).
 * @param key the message key (picks the raw or ICU reading)
 * @param english the `en` catalog value
 */
export function requirementOf(key: string, english: string): Requirement {
  const value = pickPlatformBranch(english, 'macos')
  if (isRawKey(key)) return fromRaw(value)
  try {
    return fromIcu(new IntlMessageFormat(value, 'en').getAst())
  } catch {
    return fromRaw(value)
  }
}

const QUOTES_AND_DASHES: [RegExp, string][] = [
  [/[‘’`]/g, "'"],
  [/[“”]/g, '"'],
  [/…/g, '...'],
  [/[–—]/g, '-'],
  [/[\u00a0\u202f]/g, ' '],
]

/** OCR confusables and Cyrillic look-alikes, folded on both sides so either spelling matches. */
const FOLD: Record<string, string> = {
  l: 'i',
  '1': 'i',
  '|': 'i',
  '0': 'o',
  а: 'a',
  е: 'e',
  о: 'o',
  р: 'p',
  с: 'c',
  х: 'x',
  у: 'y',
  м: 'm',
  т: 't',
  в: 'b',
  н: 'h',
  к: 'k',
}

const KEPT_PUNCTUATION = new Set(' \'".,:;!?()-/+&%[]_=@#*<>|')

/** Lowercased, quotes and dashes plain, other symbols to spaces, whitespace collapsed, look-alikes folded. */
export function normalizeOcrText(text: string): string {
  let s = text.toLowerCase()
  for (const [re, to] of QUOTES_AND_DASHES) s = s.replace(re, to)
  let out = ''
  for (const ch of s) {
    if (/[\p{L}\p{N}]/u.test(ch) || KEPT_PUNCTUATION.has(ch)) out += ch
    else out += ' '
  }
  return chars(out.split(/\s+/).filter(Boolean).join(' '))
    .map((ch) => FOLD[ch] ?? ch)
    .join('')
}

/**
 * A string's code points. Code points, not grapheme clusters, on purpose: the edit distances
 * are tuned per character of UI text, which carries no emoji sequences worth keeping whole.
 */
function chars(text: string): string[] {
  return Array.from(text)
}

/** Strips everything but letters and digits off both ends of a word. */
function trimEdge(word: string): string {
  return word.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '')
}

/** One screenshot's OCR text, prepared for matching. */
export interface OcrDoc {
  words: string[]
  wordSet: Set<string>
}

/** Prepares OCR lines for matching. A run never spans lines in the text, but may in the matcher. */
export function ocrDoc(lines: readonly string[]): OcrDoc {
  const words = lines
    .map((line) => normalizeOcrText(line))
    .join(' ')
    .split(' ')
    .map(trimEdge)
    .filter(Boolean)
  return { words, wordSet: new Set(words) }
}

/** Edit distance between two strings. */
function editDistance(a: string[], b: string[]): number {
  let prev = Array.from({ length: b.length + 1 }, (_, j) => j)
  for (let i = 1; i <= a.length; i++) {
    const cur = [i]
    for (let j = 1; j <= b.length; j++) {
      cur[j] = Math.min(prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1), prev[j] + 1, cur[j - 1] + 1)
    }
    prev = cur
  }
  return prev[b.length]
}

/** The smallest edit distance between `pattern` and any substring of `text` (Sellers). */
function substringDistance(pattern: string[], text: string[]): number {
  let prev = Array.from({ length: pattern.length + 1 }, (_, i) => i)
  let best = prev[pattern.length]
  for (const ch of text) {
    const cur = [0]
    for (let i = 1; i <= pattern.length; i++) {
      cur[i] = Math.min(prev[i - 1] + (pattern[i - 1] === ch ? 0 : 1), prev[i] + 1, cur[i - 1] + 1)
    }
    best = Math.min(best, cur[pattern.length])
    prev = cur
  }
  return best
}

/** Allowed edits per character of a longer run. */
const ERROR_RATIO = 0.08
/** Up to this many characters, a run is compared word-aligned rather than as a substring. */
const SHORT_RUN_CHARS = 12
/** Below this many characters, a short run must match exactly. */
const EXACT_RUN_CHARS = 6
/** A long run whose window wrapped or split it still counts when this share of its words show. */
const WORD_COVERAGE = 0.85

/** Whether one normalized run shows in the picture. */
export function runShows(run: string, doc: OcrDoc): boolean {
  const words = run.split(' ').map(trimEdge).filter(Boolean)
  if (words.length === 0) return true
  const joined = words.join(' ')
  const runChars = chars(joined)
  if (runChars.length <= SHORT_RUN_CHARS) {
    const allowed = runChars.length < EXACT_RUN_CHARS ? 0 : Math.max(1, Math.floor(runChars.length * ERROR_RATIO))
    for (let i = 0; i + words.length <= doc.words.length; i++) {
      const window = doc.words.slice(i, i + words.length).join(' ')
      const distance = editDistance(runChars, chars(window))
      // An inflection ("server" against "servers") is a different word, not an OCR slip.
      if (distance > 0 && (window.startsWith(joined) || joined.startsWith(window))) continue
      if (distance <= allowed) return true
    }
    return false
  }
  const allowed = Math.floor(runChars.length * ERROR_RATIO)
  if (substringDistance(runChars, chars(doc.words.join(' '))) <= allowed) return true
  const counted = words.filter((word) => chars(word).length >= 3)
  if (words.length < 6 || counted.length === 0) return false
  return counted.filter((word) => doc.wordSet.has(word)).length / counted.length >= WORD_COVERAGE
}

/** How a requirement fared: whether it holds, and how many runs it actually checked. */
interface Verdict {
  shows: boolean
  checked: number
}

function judge(req: Requirement, doc: OcrDoc, minChars: number): Verdict {
  switch (req.kind) {
    case 'literal': {
      const run = trimEdge(normalizeOcrText(req.text))
      if (chars(run).length < minChars) return { shows: true, checked: 0 }
      return { shows: runShows(run, doc), checked: 1 }
    }
    case 'all': {
      let shows = true
      let checked = 0
      for (const child of req.children) {
        const v = judge(child, doc, minChars)
        checked += v.checked
        shows &&= v.shows
      }
      return { shows, checked }
    }
    case 'any': {
      // A branch with text that shows wins. A branch with nothing to check (`=0 {}`, or only a
      // placeholder) also satisfies the choice: the picture may be showing exactly that branch.
      let failing: Verdict | null = null
      let emptyBranch = false
      for (const child of req.children) {
        const v = judge(child, doc, minChars)
        if (v.shows && v.checked > 0) return v
        if (v.shows) emptyBranch = true
        else failing ??= v
      }
      return failing === null || emptyBranch ? { shows: true, checked: 0 } : failing
    }
  }
}

/** Runs shorter than this are left out of the check while a longer one exists. */
const MIN_RUN_CHARS = 3

/**
 * Whether the picture shows the key's text: `true`, `false`, or `null` when the English has
 * no literal text to look for at all (a bare `{placeholder}`).
 */
export function showsText(key: string, english: string, doc: OcrDoc): boolean | null {
  const req = requirementOf(key, english)
  const verdict = judge(req, doc, MIN_RUN_CHARS)
  if (verdict.checked > 0) return verdict.shows
  const exact = judge(req, doc, 1)
  return exact.checked > 0 ? exact.shows : null
}
